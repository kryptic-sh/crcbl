//! Shared preparation after choosing the frame slot and uploading instances.

use super::*;

impl ForwardRenderer {
    /// Everything a frame's start does once its slot has been chosen and its
    /// instances uploaded — the shared body of [`begin_frame`](Self::begin_frame)
    /// and [`begin_skinned_frame`](Self::begin_skinned_frame).
    ///
    /// # Errors
    ///
    /// [`HalError`] on [`begin_frame`](Self::begin_frame)'s terms.
    pub(super) fn begin_frame_body(
        &mut self,
        device: &dyn Device,
        camera: &Camera,
        light: &DirectionalLight,
        target_extent: (u32, u32),
    ) -> Result<(), HalError> {
        // This slot's groups first, so everything below binds groups naming
        // the sampler in force — see `adopt_page_sampler`.
        self.adopt_page_sampler(device)?;

        // `docs/plan/39-capabilities.md`'s four layers, applied here and nowhere
        // else. Frozen for the frame because this call and `add_passes` have to
        // agree: the loops below skip a shadow cull's parameter write when
        // shadows are off, and a request changed between the two would dispatch
        // that cull against numbers nothing zeroed.
        self.frame_effects = self.resolved_effects();
        // The occlusion rung's second switch, frozen here for the field's
        // reason. Its sibling — how many planes the march sweeps — needs no
        // field: it goes straight into the uniform block written below, and the
        // block is what `add_passes` records against.
        self.frame_ssao_blurs = crate::ssao::blur_passes();

        // Requests the readback the *last* frame's copy earned and resolves the
        // slot that has come round, which is why it is here rather than beside
        // the copy: a readback covers work already submitted, and last frame's
        // copy was submitted before this call and this frame's has not been
        // recorded yet. No fence, no wait — see [`crate::cull_stats`].
        if let Some(stats) = self.cull_stats.as_mut() {
            stats.begin_frame(device);
        }

        // The internal extent: the shadow cascades' pixel scale and the atlas
        // viewer are sized by it — see [`ForwardRenderer::frame_extents`].
        let (_, extent) = self.frame_extents(target_extent);
        let direction = light.direction.normalize_or_zero();
        // `docs/plan/25-lod.md`'s two selection numbers, from this frame's
        // viewport and this frame's camera. An orthographic projection has no
        // distance falloff for the metric to divide by, so it selects under a
        // budget nothing satisfies and draws the base level whole — see
        // [`LOD_BUDGET_NONE`].
        let lod_scale = camera.projection.pixels_per_unit(extent.1 as f32);
        let lod_budget = if camera.projection.is_orthographic() {
            LOD_BUDGET_NONE
        } else {
            self.lod_error_budget
        };
        // **The cascades select from this same camera at this same scale**, and
        // differ from it in the budgets alone — `docs/plan/25-lod.md`'s shadow
        // LOD bias, and see [`SHADOW_LOD_BIAS`] for why the bias is one factor
        // over the whole pass rather than a level count.
        //
        // `LOD_BUDGET_NONE` survives the scaling, which is what an orthographic
        // camera needs: negative infinity times a positive constant is still
        // negative infinity, so a camera with no distance falloff still draws
        // the base level into the shadow map rather than a level chosen by a
        // budget the metric cannot reach.
        self.shadow_lod_params = [
            lod_scale,
            lod_budget * SHADOW_LOD_BIAS,
            lod_budget * self.lod_hold_ratio * SHADOW_LOD_BIAS,
        ];
        // Topic 18's cascades. Built from the camera and the light alone, so a
        // frame that culls against them and a fragment that samples through them
        // cannot disagree about where they are.
        let cascades = Cascades::new(camera, direction);
        // **Cascade 0's own reach, kept for the reflective shadow map.** The map
        // is that cascade's box, so how much world one of its texels covers is a
        // function of this number — see `crate::rsm::texel_area`. Read here
        // rather than at pass-record time for the reason the matrices are: a
        // frame that scaled its gather against one fitting while it drew the map
        // through another would be a bounce nothing in the frame could see was
        // wrong.
        self.cascade_reach = cascades.far[0];
        // And the sun the map is drawn through, as the light table carries it.
        // The map holds albedo rather than flux — `RsmOutput` in `mesh.slang`
        // argues why — so this is what the gather multiplies by.
        self.sun_color = light.color;
        let mut shadow_view_proj = [[0.0f32; 16]; shadow::CASCADES];
        for (matrix, cascade) in shadow_view_proj.iter_mut().zip(&cascades.view_proj) {
            *matrix = cascade.to_cols_array();
        }
        // **Topic 18's light list, and the sun is row 0 of it.** Its direction
        // and colour were two fields of the block below until the list existed;
        // `sun_row` normalises exactly as this function did, so the row carries
        // the same bits and `mesh.slang`'s loop over one directional light is
        // the same arithmetic the single-light form performed. The goldens are
        // what say so.
        //
        // Rebuilt every frame rather than kept: the sun arrives per frame, and a
        // cached list would be a second place for it to be stale.
        //
        // **The tile budget is spent first**, because a row carries the tile it
        // was given. Topic 18's rule — projected screen influence, ties by index,
        // an incumbent held until a challenger clearly beats it — lives in
        // `shadow::Selection`, and a light it refuses is a light whose row says
        // `NO_SHADOW_TILE`: it still lights, it just does not occlude.
        self.shadow_lights.update(&self.extra_lights, camera);
        let mut rows = Vec::with_capacity(1 + self.extra_lights.len());
        rows.push(sun_row(light));
        rows.extend(
            self.extra_lights
                .iter()
                .enumerate()
                .map(|(index, extra)| extra.row(self.shadow_lights.base_of(index))),
        );

        // And where each of those maps went in the atlas, out of the allocator
        // the selection above just spent: a scale and an offset per slot, which
        // is the whole of what the sampling side knows about the image's shape.
        // Read once and handed to both blocks that carry it — the frame's and
        // the froxel volume's — because a froxel reading one frame's rectangle
        // against another's matrix would sample the right map's place for the
        // wrong map.
        let atlas_rects = self.shadow_lights.atlas_rects();

        // One matrix per held light tile, and the identity in a free one — a
        // spot fills the one tile it was given and a point light the six from
        // its base, in `shadow::face_axis`' order, which is the order
        // `mesh.slang`'s `point_face` selects between. The identity is not a
        // projection anything samples through — the rows that could name a free
        // tile carry `NO_SHADOW_TILE` — and it is written rather than left stale
        // so a block dumped for debugging says plainly that the tile is empty.
        let mut light_view_proj = [Mat4::IDENTITY.to_cols_array(); shadow::LIGHT_TILES];
        for assignment in self.shadow_lights.slots().iter().flatten() {
            match self.extra_lights.get(assignment.light) {
                Some(Light::Spot(spot)) => {
                    light_view_proj[assignment.base] = shadow::spot_matrix(spot).to_cols_array();
                }
                Some(Light::Point(point)) => {
                    for face in 0..shadow::POINT_FACES {
                        light_view_proj[assignment.base + face] =
                            shadow::point_matrix(point, face).to_cols_array();
                    }
                }
                // A rectangle is refused a tile by `shadow::can_be_shadowed`,
                // so it never holds a slot and never reaches this — the arm is
                // here because the alternative is a wildcard that would also
                // swallow the next kind of light somebody adds.
                Some(Light::Rect(_)) | None => {}
            }
        }

        // The frame's half of every view's frame block, kept for the frame so a
        // secondary view begun after this call writes its blocks from exactly
        // what the primary camera's were — see [`ForwardRenderer::begin_view`].
        self.frame_serial = self.frame_serial.wrapping_add(1);
        let scene = FrameScene {
            light: *light,
            rows,
            cascades,
            shadow_view_proj,
            light_view_proj,
            atlas_rects,
        };

        // An atmosphere's LUT is marched here rather than in the pass that
        // draws it, and before any view's blocks are written: every view reads
        // the sky this leaves presented — see [`ForwardRenderer::view_frame`] —
        // and the march is the expensive part, so it runs once for the frame.
        self.refresh_sky_view();
        // Every element the pool has ever handed out, not its live count: a
        // removed instance leaves a hole and the live ones above it still have
        // to be tested. `InstancePool::slot_count` carries the difference.
        let instance_count = self.instances.slot_count();
        // The camera and the two selection numbers go to the cull/draw-argument
        // pair as well as into the block above, and they are handed over rather
        // than re-derived: `docs/plan/25-lod.md`'s uniform cut runs there, the
        // mesh path's per-cluster descent runs off the block, and a frame that
        // selected detail against one camera while drawing with another is a
        // difference nothing in the frame can see.
        //
        // **The eye is the selection's, which is the camera's unless a caller
        // pinned it** — `set_frozen_selection_eye`, and the whole of what that
        // feature is. It reaches this parameter block and nothing else: the
        // frustum handed over with it is extracted from this frame's own
        // view-projection, and the frame block written above carries
        // `camera.eye`, so a pinned selection changes which cut is chosen and
        // nothing about what is culled, faced or drawn.
        let selection_eye = self.frozen_selection_eye.unwrap_or(camera.eye);
        let frame = self.view_frame(&scene, target_extent, instance_count, selection_eye);
        let uniforms = self
            .primary
            .begin_frame(device, camera, &frame, self.sky_view.as_ref())?;

        // The atlas viewer's block: where the atlas is letterboxed into this
        // frame, and where each slot's map is inside it. Written on the four
        // above's terms — a frame not drawing the view pays for the write and
        // reads none of it, and a block written only on the frames that draw it
        // is stale on the frame a caller switches the view on.
        //
        // **`atlas_rects` and not a second derivation of it.** These are the
        // rectangles the frame block above carries, so the grid the viewer draws
        // is the one the sampling side reads through; a viewer that worked its
        // own out would be a diagnostic supplying its own evidence.
        self.atlas_viewer.begin_frame(
            device,
            self.frame,
            extent,
            shadow::atlas_extent(),
            scene.atlas_rects,
        )?;

        // `docs/plan/07-ui-debug.md` item 5's immediate-mode buffer: whatever
        // any system appended since the last frame, uploaded and cleared here.
        // **The camera this frame is drawn with**, not a second derivation of
        // it — the ground grid's pass takes the same matrix for the same
        // reason.
        //
        // A frame with nothing appended, or one with the console switch off,
        // uploads nothing and leaves the slot's count at zero, which is what
        // makes `add_frame_passes` record no pass at all. See
        // [`crate::debug_draw`].
        let frames = self.primary.uniforms.len();
        self.debug_draw
            .begin_frame(device, frames, self.frame, self.primary.camera_view_proj)?;
        // The water's buffers for this slot, if the bodies changed since the
        // slot last came round — see [`crate::water::WaterBodies`], which says
        // why this waits for the slot rather than happening in `set_water`.
        self.water.begin_frame(device, self.frame)?;
        // The grass field's retirements, aged by one frame — see
        // [`crate::grass::GrassScene`], which says why a replaced field waits
        // for the ring rather than being released where it was replaced.
        self.grass.begin_frame(device);

        // One cull per cascade and per **occupied** light slot, against that
        // view's own frustum. The orthographic box gives
        // `Frustum::from_view_projection` six real planes — unlike the camera's
        // infinite perspective, whose far plane is degenerate on purpose — so a
        // caster outside the cascade is rejected before it costs a vertex. A
        // spot's perspective box is finite too, and gives six real planes for
        // the same reason.
        //
        // **A point light culls against its sphere rather than against a
        // matrix**, which is topic 18's fourth decision: its six faces have six
        // matrices and one visible set between them, so what they share is the
        // reach of the light. `shadow::point_frustum` is that box.
        //
        // A free slot takes neither a write nor a dispatch: nothing samples
        // through its tiles, and `add_shadow_pass` records no pass for it
        // either, so they keep the reversed-Z clear the pass wrote.
        //
        // **A frame with [`RenderEffects::SHADOWS`] off is every slot free**,
        // and the whole block below is skipped for the same reason — the atlas
        // is cleared and nothing draws into it, so there is no cull to parametrise
        // and no view to write a matrix for. That is what makes the switch cost
        // nothing rather than costing the culls and throwing them away.
        //
        // **The camera as the eye handed to `begin_frame`, not the light**, and
        // the two are deliberately different questions asked of one pass.
        //
        // The block each view writes puts the light at `camera_position` because
        // the amplification stage's *normal cone* test asks which way a cluster
        // faces relative to the viewer, and a shadow map's viewer is the light.
        // Detail is not that question. A directional sun has no position for a
        // distance metric to measure from — the point below is the camera's own
        // eye pushed along the sun's direction, so a "distance to the light"
        // taken from it is a fact about the camera wearing the light's name, and
        // it steps discontinuously from one cascade to the next because
        // `scene.cascades.far` does.
        //
        // What a coarser caster actually costs is a shadow edge displaced by the
        // group's error, and that displacement is **seen by the camera**, at the
        // camera's pixels per unit and the camera's distance. So the budget is
        // denominated in the camera's pixels, and the eye that makes the metric
        // mean that is the camera's. The bias above is then a statement about
        // shadows rather than a side effect of where a light was placed — and it
        // is the same statement for a spot or a point light, whose maps are
        // looked at through the camera's pixels just as a cascade's is.
        // `docs/plan/45-shadows.md`'s static-caching rung, and the whole of the
        // decision it makes: everything below is assembled first and written
        // only if it differs from what the atlas was last *drawn* from. See
        // [`ForwardRenderer::shadow_atlas_record`] for what the comparison
        // covers and what it deliberately does not.
        let shadows = self.frame_effects.contains(RenderEffects::SHADOWS);
        // The cascades select for the camera, so they take the camera's
        // selection eye — pinned along with the colour pass's, or a frozen cut
        // would draw under a shadow silhouette that was still following the
        // reviewer around.
        let eye = [selection_eye.x, selection_eye.y, selection_eye.z];
        let view_block = |view_proj: Mat4, from: Vec3| -> mesh::FrameUniforms {
            // The spread carries the normals view's switch into these blocks too,
            // and nothing reads it: `MeshModules::depth_pipeline` names no
            // fragment stage at all, so the atlas is filled by the geometry
            // stages alone whichever view the colour pass is drawing.
            mesh::FrameUniforms {
                view_proj: view_proj.to_cols_array(),
                camera_position: from.extend(1.0).to_array(),
                // **This view's budgets, not the camera's**, which is the pair
                // its own draw generator selected under —
                // [`shadow_lod_params`](Self::shadow_lod_params). Nothing reads
                // the colour these stages produce, because `depth_pipeline`
                // names no fragment stage; carrying the camera's numbers here
                // would still be a block that says a cascade selected under a
                // budget it did not.
                lod_params: [
                    self.shadow_lod_params[0],
                    self.shadow_lod_params[1],
                    self.shadow_lod_params[2],
                    0.0,
                ],
                // **This view's own matrix, so the interpolant is zero motion**
                // rather than a reprojection through the camera. Nothing reads
                // it: `depthVertexMain` never looks at it, and where the mesh
                // stage draws these tiles instead, the previous clip position it
                // emits dies with the fragment stage `depth_pipeline` does not
                // name. Carrying the camera's matrix would still be a block that
                // says a cascade moved the way the viewer did.
                previous_view_proj: view_proj.to_cols_array(),
                // **Not the console's filter row.** `depth_pipeline` names no
                // fragment stage, so nothing drawing these views reads it — and
                // the block's bytes are the atlas cache's record of what a map
                // was drawn from. Carried, the row made `r_shadow_filter` and
                // `r_shadow_split` redraw every map in the atlas, and made the
                // tests that move them redraw it under the cache tests in this
                // crate's own process. Zeroed, a sampling knob is a sampling
                // knob. `a_moved_shadow_filter_leaves_a_still_atlas_alone` holds
                // it there.
                shadow_filter: [0; 4],
                // **Nor the console's bias row**, for the same reason: the
                // two counts `Cascades::params` reads out of `r_shadow_bias`
                // and `r_shadow_normal_offset` decide how far from a receiver
                // the colour pass samples a map, and nothing drawing these
                // views reads them. The atlas's two texel sizes beside them
                // are the layout's and never move, so they stay. The same test
                // holds this row at zero.
                shadow_params: [
                    uniforms.shadow_params[0],
                    uniforms.shadow_params[1],
                    0.0,
                    0.0,
                ],
                ..uniforms
            }
        };
        // Which view gets which block, and which cull is parametrised with
        // which frustum — **built before either is written**, because the
        // record below has to be exactly what the atlas would be drawn from
        // rather than a second derivation of it. A frame with shadows off fills
        // neither: nothing draws into the atlas, so the pass's clear is the
        // whole of what it will hold.
        //
        // **Grouped**, since the cadence rung: a group is a cascade or a light
        // slot's whole run of tiles, it is what one cull covers, and it is what
        // [`shadow::Cadence`] schedules. `regions` is the tier each group sits
        // on, the centre its maps are projected from and how far they reach —
        // the three things the schedule and the reset are decided from.
        let mut views: Vec<(usize, usize, mesh::FrameUniforms)> = Vec::with_capacity(SHADOW_VIEWS);
        let mut culls: Vec<(usize, Frustum)> = Vec::with_capacity(SHADOW_CULLS);
        // A point light's six faces' side planes, per cull, where the light is
        // one and the face culls are on — see `set_point_face_culls`. Beside
        // `culls` rather than in it, because that list is also what the shadow
        // cache's record is folded from, and the faces are already in it as the
        // light's matrices.
        let mut face_culls: [Option<crate::cull::FacePlanes>; SHADOW_CULLS] = [None; SHADOW_CULLS];
        let mut regions: [Option<(usize, Vec3, f32)>; SHADOW_CULLS] = [None; SHADOW_CULLS];
        // One shadowed light slot's per-face matrices and the frustum its one
        // cull runs against, read out of the selection made above.
        //
        // **The one derivation both paths below read.** The atlas's fitting
        // loop takes it, and so does the shadows-off branch that keeps
        // `docs/plan/50-irradiance-probes.md`'s punctual producer fed — so a
        // face drawn into the atlas and the same face drawn into the reflective
        // shadow map cannot be through different matrices.
        let slot_matrices = |held: shadow::Assignment, light: &Light| -> (Vec<Mat4>, Frustum) {
            let faces = (0..shadow::tile_span(light))
                .map(|face| Mat4::from_cols_array(&scene.light_view_proj[held.base + face]))
                .collect();
            let frustum = match light {
                Light::Point(point) => shadow::point_frustum(point),
                // A rectangle holds no slot, for the reason the
                // `light_view_proj` fill above gives, so it cannot reach either
                // caller; its tile's identity matrix is what this would cull
                // against if it did.
                Light::Spot(_) | Light::Rect(_) => Frustum::from_view_projection(
                    Mat4::from_cols_array(&scene.light_view_proj[held.base]),
                ),
            };
            (faces, frustum)
        };
        if shadows {
            for (cascade, region) in regions.iter_mut().take(shadow::CASCADES).enumerate() {
                let view_proj = scene.cascades.view_proj[cascade];
                let centre = camera.eye + direction * scene.cascades.far[cascade];
                views.push((cascade, cascade, view_block(view_proj, centre)));
                culls.push((cascade, Frustum::from_view_projection(view_proj)));
                // **The cascade's own index is its tier**, which is the cadence
                // rung's own words: the near cascade is re-rendered every frame
                // and each one out doubles the period. Its reach is the sphere
                // it was fitted to — see [`shadow::Cascades::far`] — so an eye
                // that has moved further than that is an eye whose new sphere
                // the held map does not reach at all.
                *region = Some((cascade, centre, scene.cascades.far[cascade]));
            }
            for (slot, held) in self.shadow_lights.slots().iter().enumerate() {
                let Some(held) = held else {
                    continue;
                };
                // A selection's indices are into the list it was run over, which
                // is this one — so this is a resolution rather than a check.
                // Skipped rather than asserted all the same, because the
                // alternative to a frame with one shadow missing is no frame at
                // all.
                let Some(light) = self.extra_lights.get(held.light) else {
                    continue;
                };
                let group = shadow_cull(slot);
                let (faces, frustum) = slot_matrices(*held, light);
                face_culls[group] = self.point_face_planes(light, &faces);
                for (face, view_proj) in faces.into_iter().enumerate() {
                    views.push((
                        group,
                        shadow_view(slot, face),
                        view_block(view_proj, light.sphere().0),
                    ));
                }
                culls.push((group, frustum));
                // **The tile's own level is the tier**, so `shadow::coverage`
                // decides how often a light's map is redrawn exactly as it
                // decides how large that map is — one scorer, read twice, which
                // is what the priority rung established. The reach is the
                // light's own radius: a light that has moved further than that
                // lights somewhere its held map says nothing about.
                let (centre, reach) = light.sphere();
                regions[group] = Some((held.level, centre, reach));
            }
        }

        // The frame the cadence is keyed to, and the one thing in this whole
        // decision that is not a function of the scene.
        self.shadow_frame = self.shadow_frame.wrapping_add(1);

        // The pass recorded last, if its body ran: what it drew is now what the
        // image holds. Applied here rather than where it was recorded, for
        // [`ForwardRenderer::shadow_pass_ran`]'s reason — a frame whose graph
        // was refused left the image exactly as it was.
        if let Some(pending) = self.shadow_pending.take()
            && self.shadow_pass_ran.load(Ordering::Relaxed) == pending.id
        {
            for (group, held) in pending.groups.iter().enumerate() {
                if let Some(held) = *held {
                    self.shadow_group_held[group] = held;
                }
            }
            if pending.layout.is_some() {
                self.shadow_atlas_layout = pending.layout;
            }
        }

        // Where every map lands this frame. **A frame with shadows off is one
        // more layout rather than a second kind of state**: the atlas holds a
        // clear instead of maps, so its layout is the empty one, and the
        // comparison below is what makes the frame that switches shadows off
        // clear the image once and the frames after it cost nothing.
        let layout: Vec<shadow::TileRect> = (0..shadow::TILES)
            .map(|slot| {
                if shadows {
                    self.shadow_lights.atlas_rect(slot)
                } else {
                    shadow::TileRect::EMPTY
                }
            })
            .collect();
        // **A layout that has moved resets the whole atlas.** A tile left over
        // from a different layout is texels nothing this frame can account for,
        // and the only clear this seam has covers the whole attachment — so such
        // a frame clears everything and redraws every group, with no cadence and
        // no budget deciding otherwise.
        let relaid = self.shadow_atlas_layout.as_ref() != Some(&layout);

        // What each group would be drawn from, and which of them the image is
        // already holding.
        let mut wanted: [Option<shadow::Group>; SHADOW_CULLS] = [None; SHADOW_CULLS];
        for (group, region) in regions.iter().enumerate() {
            let Some((tier, centre, _)) = *region else {
                continue;
            };
            let record = self.shadow_group_record(group, &views, &culls, eye, instance_count);
            if record != self.shadow_group_inputs[group] {
                self.shadow_group_inputs[group] = record;
                self.shadow_group_id[group] = self.shadow_group_id[group].wrapping_add(1);
            }
            let (held_id, held_centre, held_reach) = self.shadow_group_held[group];
            // A skinned frame's vertices are written by a compute pass into the
            // pool, so nothing on the host can tell this group's maps from the
            // last frame's — see [`ForwardRenderer::frame_skins`].
            //
            // **No group is held while the probe updater is on**, and that is a
            // correctness rule rather than a cost: the reflective shadow maps
            // are drawn from these groups' own draws, and the probe table is a
            // ring of `FRAMES_IN_FLIGHT` buffers. A frame that skipped one
            // producer would write *this frame's* slot without it, and the next
            // frame's would have it again — so the room would light and unlight
            // on alternate frames. It is cascade 0's rule extended to the light
            // slots by the punctual producer, which draws through their views
            // exactly as the sun's half draws through cascade 0's.
            //
            // Redrawing a held group costs its tiles and draws exactly the
            // texels they already held, so the atlas is unchanged; what it buys
            // is a map for the gather to read.
            let updater_needs_it = self.probe_update_runs();
            if !relaid
                && !self.frame_skins
                && !updater_needs_it
                && held_id == self.shadow_group_id[group]
            {
                continue;
            }
            wanted[group] = Some(shadow::Group {
                faces: views.iter().filter(|(owner, _, _)| *owner == group).count(),
                tier,
                // The reset, per group: a map whose centre has moved further
                // than its own reach is about somewhere else rather than a
                // frame out of date. A group the image has never held reads as
                // forced too, its reach being zero, which is the right answer
                // for the first frame of all.
                forced: relaid || centre.distance(held_centre) > held_reach,
            });
        }

        // **The reset spends no budget**: a relaid atlas has nothing to hold, so
        // every group it still has is redrawn whatever the console says.
        let cadence = if relaid {
            shadow::Cadence::EVERY_FRAME
        } else {
            self.shadow_cadence_request
                .unwrap_or_else(shadow::Cadence::from_console)
        };
        let redraw = cadence.schedule(self.shadow_frame, &wanted);
        self.shadow_group_redrawn.copy_from_slice(&redraw);
        self.shadow_faces_redrawn = wanted
            .iter()
            .enumerate()
            .filter(|(group, _)| redraw[*group])
            .filter_map(|(_, want)| *want)
            .map(|want| u32::try_from(want.faces).unwrap_or(u32::MAX))
            .sum();
        self.shadow_cadence_reset = wanted.iter().flatten().any(|want| want.forced);
        // **Whether the pass clears the whole attachment or loads it.** Clearing
        // is what shipped and is what a frame keeping nothing should do — every
        // group it draws at all, it redraws. Loading is the cadence's own path,
        // and there each redrawn tile is reset by
        // [`MeshModules::depth_clear_pipeline`] instead.
        self.shadow_atlas_clears = relaid
            || regions
                .iter()
                .enumerate()
                .all(|(group, region)| region.is_none() || redraw[group]);
        // A relaid atlas records its pass even with nothing to draw into it: the
        // clear *is* the frame's answer, and it is what a frame that switched
        // shadows off leaves behind for anything that samples the maps.
        // **A frame drawing without `RenderEffects::SHADOWS` still fits cascade
        // 0 while the probe updater is on.** The reflective shadow map is that
        // cascade's box and its draws, and it is not the atlas: nothing here
        // puts cascade 0 into `views` or `regions`, so no tile is written and
        // the switch goes on removing every shadow in the frame. What it buys is
        // that switching shadows off does not also switch the *bounce* off,
        // which would make the sun's direct term and its indirect one one
        // toggle — and `apps/lantern`'s `every_effect_toggles_and_the_frame_says_so`
        // is what says a floor already in full sun must not move when the
        // shadows go.
        let updater_drives_cascade_zero = !shadows && self.probe_update_runs();
        self.shadow_atlas_cached =
            !relaid && !redraw.iter().any(|drawn| *drawn) && !updater_drives_cascade_zero;
        if self.shadow_atlas_cached {
            // Nothing was fitted, so there is nothing for the map to be drawn
            // from — see [`ForwardRenderer::rsm_cull_ready`].
            self.rsm_cull_ready = false;
            self.frame_scene = Some(scene);
            return Ok(());
        }

        self.shadow_pass_id = self.shadow_pass_id.wrapping_add(1);
        self.shadow_pending = Some(ShadowCommit {
            id: self.shadow_pass_id,
            groups: regions
                .iter()
                .enumerate()
                .map(|(group, region)| {
                    let (_, centre, reach) = (*region)?;
                    redraw[group].then_some((self.shadow_group_id[group], centre, reach))
                })
                .collect(),
            layout: self.shadow_atlas_clears.then_some(layout),
        });

        for (group, view, block) in &views {
            if !redraw[*group] {
                continue;
            }
            device.write_buffer(
                self.shadow_uniforms[self.frame][*view],
                0,
                &block.to_bytes(),
            )?;
        }
        for (cull, frustum) in &culls {
            if !redraw[*cull] {
                continue;
            }
            self.shadow_draws[*cull].begin_frame_with(
                device,
                self.frame,
                frustum,
                instance_count,
                crate::draw_gen::Selection {
                    camera_position: eye,
                    lod_params: self.shadow_lod_params,
                },
                &face_culls[*cull]
                    .as_ref()
                    .map_or(FrameCull::Plain, FrameCull::Faces),
            )?;
        }
        // Cascade 0's block and cull on a frame with shadows off, and every
        // shadowed light's beside it, which the two loops above skipped because
        // they are in neither list — see `updater_drives_cascade_zero`. Written
        // straight rather than pushed into `views`/`culls`, because those are
        // what the *atlas* is drawn from and none of this is being drawn into
        // it.
        if updater_drives_cascade_zero {
            let view_proj = scene.cascades.view_proj[0];
            let centre = camera.eye + direction * scene.cascades.far[0];
            device.write_buffer(
                // **Cascade 0's view is view 0.** `shadow_view` numbers a light
                // slot's faces, which start past `shadow::CASCADES` — a cascade
                // is its own index, which is what the loop above pushes.
                self.shadow_uniforms[self.frame][CASCADE_ZERO_VIEW],
                0,
                &view_block(view_proj, centre).to_bytes(),
            )?;
            self.shadow_draws[0].begin_frame(
                device,
                self.frame,
                &Frustum::from_view_projection(view_proj),
                instance_count,
                eye,
                self.shadow_lod_params,
            )?;
            // And the punctual producer's views, on exactly the terms above:
            // the lamp's bounce is drawn through the very faces the atlas would
            // have used, so a frame that fitted neither would switch the
            // *bounce* off with the shadows — which is the one thing the block
            // above exists to prevent, and `apps/lantern`'s frame claim 6 is
            // what would say so.
            for (slot, held) in self.shadow_lights.slots().iter().enumerate() {
                let Some(held) = *held else {
                    continue;
                };
                let Some(light) = self.extra_lights.get(held.light) else {
                    continue;
                };
                let (faces, frustum) = slot_matrices(held, light);
                let face_planes = self.point_face_planes(light, &faces);
                for (face, view_proj) in faces.into_iter().enumerate() {
                    device.write_buffer(
                        self.shadow_uniforms[self.frame][shadow_view(slot, face)],
                        0,
                        &view_block(view_proj, light.sphere().0).to_bytes(),
                    )?;
                }
                self.shadow_draws[shadow_cull(slot)].begin_frame_with(
                    device,
                    self.frame,
                    &frustum,
                    instance_count,
                    crate::draw_gen::Selection {
                        camera_position: eye,
                        lod_params: self.shadow_lod_params,
                    },
                    &face_planes
                        .as_ref()
                        .map_or(FrameCull::Plain, FrameCull::Faces),
                )?;
            }
        }
        // Whether the map has a cascade to be drawn from at all: the updater is
        // on, and cascade 0's block and cull were written this frame — by the
        // loops above where shadows are on, or by the branch above where they
        // are off.
        self.rsm_cull_ready = self.probe_update_runs()
            && (updater_drives_cascade_zero || (redraw[0] && regions[0].is_some()));
        // `docs/plan/50-irradiance-probes.md`'s gather, parameterised from the
        // cascade this frame just fitted and the sun it was fitted to. Written
        // here rather than in the pass body: a pass body runs while the frame's
        // commands are being recorded, and a host write to a buffer an earlier
        // submission may still be reading is the hazard the whole ring exists to
        // avoid.
        //
        // **Written whether or not the pass is recorded.** Which slot the gather
        // reads is decided by the frame, and a block left holding an older
        // frame's sun would be read the moment the pass came back.
        //
        // **The producer table is written here too**, off the selection this
        // call has just made: `add_shadow_pass` draws exactly this list and the
        // gather walks exactly this list, and `punctual_faces` is what makes
        // those the same sentence.
        let producers: Vec<PunctualProducer> = self
            .punctual_faces()
            .iter()
            .map(|face| face.producer)
            .collect();
        if let Some(gather) = self.probe_gather.as_ref() {
            gather.begin_frame(
                device,
                self.frame,
                self.sun_color.to_array(),
                rsm::texel_area(self.cascade_reach),
                &producers,
            )?;
        }
        self.frame_scene = Some(scene);
        Ok(())
    }
}
