//! Construction of the built-in screenshot scenes.

use super::*;

impl SceneState {
    /// Builds the renderer this scene needs, and uploads whatever it draws.
    pub(super) fn open(
        scene: Scene,
        device: &dyn Device,
        queue: QueueHandle,
        format: Format,
        path: Option<crate::hal::GeometryPath>,
    ) -> Result<Self, OffscreenError> {
        let path = path.unwrap_or_else(|| device.preferred_geometry_path());
        Ok(match scene {
            Scene::Cube => {
                let mut renderer =
                    forward_renderer(device, queue, format, &crate::render::scene::demo(), path)?;
                place_cube(&mut renderer, ForwardRenderer::spin(0.0));
                place_pyramids(&mut renderer);
                Self::Forward {
                    camera: Camera::default().with_projection(Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.01,
                    }),
                    light: DirectionalLight::default(),
                    renderer,
                }
            }
            Scene::Lights => {
                // The cube scene's geometry exactly, so the two goldens differ
                // in their light lists and in nothing else.
                let mut renderer =
                    forward_renderer(device, queue, format, &crate::render::scene::demo(), path)?;
                place_cube(&mut renderer, ForwardRenderer::spin(0.0));
                place_pyramids(&mut renderer);
                renderer.set_lights(&scene_lights());
                Self::Forward {
                    camera: Camera::default().with_projection(Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.01,
                    }),
                    light: dim_sun(),
                    renderer,
                }
            }
            Scene::Spot => {
                // **The cube alone, and it is the floor.** Every other resident
                // stays off: a pyramid beside the pool would be a second lit
                // shape in a frame whose whole content is meant to be one cone
                // on one flat surface.
                let mut renderer =
                    forward_renderer(device, queue, format, &crate::render::scene::demo(), path)?;
                place_cube(&mut renderer, spot_floor());
                renderer.set_lights(&[spot_light()]);
                Self::Forward {
                    camera: spot_camera(),
                    light: spot_sun(),
                    renderer,
                }
            }
            Scene::SpotShadow => {
                // The spot scene's floor exactly, with one thing added: the
                // pyramid, standing in the light. Everything else that differs
                // — the camera, the light's tilt — is what makes the shadow
                // separable from its caster, and `Scene::SpotShadow` is where
                // that is argued.
                let mut renderer =
                    forward_renderer(device, queue, format, &crate::render::scene::demo(), path)?;
                place_cube(&mut renderer, spot_floor());
                place(
                    &mut renderer,
                    DEMO_PYRAMID,
                    DEMO_UNTINTED,
                    spot_shadow_caster(),
                );
                renderer.set_lights(&[spot_shadow_light()]);
                Self::Forward {
                    camera: spot_shadow_camera(),
                    light: spot_sun(),
                    renderer,
                }
            }
            Scene::PointShadow => {
                // The spot-shadow scene's floor, with **two** casters on
                // different sides of the light: one out along `+X` and one out
                // along `-Z`, so their shadows fall across two different faces
                // of the light's map. One caster would prove a point light casts
                // *a* shadow, which is what a single working face already does.
                let mut renderer =
                    forward_renderer(device, queue, format, &crate::render::scene::demo(), path)?;
                place_cube(&mut renderer, spot_floor());
                place(
                    &mut renderer,
                    DEMO_PYRAMID,
                    DEMO_UNTINTED,
                    point_shadow_caster(glam::Vec3::new(POINT_CASTER_AT, 0.0, 0.0)),
                );
                place(
                    &mut renderer,
                    DEMO_PYRAMID,
                    DEMO_TINTED,
                    point_shadow_caster(glam::Vec3::new(0.0, 0.0, -POINT_CASTER_AT)),
                );
                renderer.set_lights(&[point_shadow_light()]);
                Self::Forward {
                    camera: point_shadow_camera(),
                    light: spot_sun(),
                    renderer,
                }
            }
            Scene::AreaLight => {
                // **The floor alone, and it is the cube.** Nothing stands on it,
                // for `Scene::Spot`'s reason and one sharper: an area light's
                // whole difference from a point light is the *shape* of its
                // highlight, and a second object in the frame is something a
                // reader — or an assertion — can take that shape off instead.
                // The cube is placed rather than parked, so it is still the
                // first insertion and still holds the pool slot every other
                // forward scene gives it, and it is placed through the dark
                // glossy row `area_scene` appends for it.
                let mut renderer = forward_renderer(device, queue, format, &area_scene(), path)?;
                place(&mut renderer, DEMO_CUBE, AREA_FLOOR, spot_floor());
                // **The reflection pair, refused**, on `Scene::Probes`' terms
                // and with a sharper exposure: this floor carries
                // `PYRAMID_ROUGHNESS`, which is under `ssr.slang`'s cutoff, so a
                // march over the depth buffer would write into exactly the
                // pixels the highlight is measured in. It would also put this
                // scene in `tests/render_e2e.rs`'s `path_lsb_channels` budget
                // for a term the scene is not about — with the pair refused, the
                // two geometry paths draw this frame byte for byte.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                renderer.set_lights(&area_strips());
                Self::Forward {
                    camera: area_camera(),
                    light: area_sun(),
                    renderer,
                }
            }
            Scene::FillLight => {
                // `Scene::AreaLight`'s floor, placed the same way and through
                // the same row, and one more reason of this scene's own for the
                // floor being the only thing in it: a fill light is refused a
                // shadow tile and its lit twin is handed one, so a caster here
                // would put a shadow on one half of the mirror and nothing on
                // the other, and the difference the bands measure would be a
                // shadow wearing the fill flag's name.
                let mut renderer = forward_renderer(device, queue, format, &area_scene(), path)?;
                place(&mut renderer, DEMO_CUBE, AREA_FLOOR, spot_floor());
                // **The reflection pair, refused**, on `Scene::AreaLight`'s
                // terms exactly: this is that floor and it carries that
                // roughness, which is under `ssr.slang`'s cutoff, so a march
                // over the depth buffer would write into the pixels the
                // highlights are measured in.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                renderer.set_lights(&fill_light_pairs());
                Self::Forward {
                    camera: area_camera(),
                    light: area_sun(),
                    renderer,
                }
            }
            Scene::AlphaMask => {
                // **The floor first, and the plate over it.** The cube is
                // placed rather than parked, so it is still the first insertion
                // and still holds the pool slot every other forward scene gives
                // it — see `place`, where insertion order is argued — and it is
                // placed through the grey row `alpha_mask_scene` appends for it
                // rather than through `DEMO_UNTINTED`, which is why there is no
                // `place_cube` call.
                //
                // The plate is the second and last: nothing else is in the
                // frame, for `Scene::Spot`'s reason and one sharper. Two of this
                // scene's three claims are read off bands of *floor* — one seen
                // through the hole, one in the shadow's hole — and any other
                // object standing on that floor is something a band could be
                // measuring instead.
                let mut renderer =
                    forward_renderer(device, queue, format, &alpha_mask_scene(), path)?;
                place(&mut renderer, DEMO_CUBE, ALPHA_FLOOR, alpha_floor());
                place(&mut renderer, DEMO_CUBE, ALPHA_PLATE, alpha_plate());
                Self::Forward {
                    camera: alpha_camera(),
                    light: alpha_sun(),
                    renderer,
                }
            }
            Scene::DoubleSided => {
                // **The floor first, and the three quads over it**, on
                // `Scene::AlphaMask`'s terms exactly: the cube is placed rather
                // than parked so it holds the pool slot every forward scene
                // gives it, and through the grey row `double_sided_scene`
                // appends for it rather than through `DEMO_UNTINTED`.
                //
                // Nothing else is in the frame. Four of this scene's bands are
                // read off *floor* — one where a culled quad is not, one of open
                // lit floor, and two inside shadows — and any other object
                // standing on that floor is something a band could be measuring
                // instead.
                let mut renderer =
                    forward_renderer(device, queue, format, &double_sided_scene(), path)?;
                place(&mut renderer, DEMO_CUBE, DOUBLE_FLOOR, double_floor());
                for (material, model) in double_sided_quads() {
                    place(&mut renderer, DOUBLE_QUAD_MESH, material, model);
                }
                Self::Forward {
                    camera: double_camera(),
                    light: double_sun(),
                    renderer,
                }
            }
            Scene::SpecularAa => {
                let mut renderer =
                    forward_renderer(device, queue, format, &specular_aa_scene(), path)?;
                // **Nothing marches over this frame.** The plate is smoother
                // than `ssr.slang`'s cutoff, so a screen-space reflection pass
                // would compose its own answer into both bands — and what the
                // bands are evidence about is one lobe evaluated in one stage.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                // The plate alone: nothing else may stand in a band, and there
                // is no floor for its shadow to fall on either — a caster over
                // an empty frame is the whole scene, which is what leaves the
                // margin either side of the plate readable as background.
                place(
                    &mut renderer,
                    SPECULAR_PLATE_MESH,
                    SPECULAR_MATERIAL,
                    specular_plate(),
                );
                Self::Forward {
                    camera: specular_camera(),
                    light: specular_sun(),
                    renderer,
                }
            }
            Scene::Dunes => {
                let mut renderer =
                    forward_renderer(device, queue, format, &crate::render::scene::demo(), path)?;
                place_cube(&mut renderer, ForwardRenderer::spin(0.0));
                // **Refused rather than drawn empty.** `selects_levels` says no
                // on a device that reports a mesh stage and no amplification
                // stage, where the un-amplified path would emit every cluster of
                // every level at once; every other device draws the patch, per
                // cluster or through a uniform cut. Asking before placing the
                // patch is the caller's job — `add_instance` has no vocabulary
                // for that refusal — and not asking is what made this scene a
                // frame of clear colour that a golden would have been blessed
                // from.
                if !renderer.selects_levels() {
                    renderer.destroy(device);
                    return Err(OffscreenError::Unusable(
                        "this device reports a mesh stage and no amplification stage, so the \
                         dunes patch's cluster DAG has nothing that can select a level in it",
                    ));
                }
                place(
                    &mut renderer,
                    DEMO_DUNES,
                    DEMO_UNTINTED,
                    glam::Mat4::IDENTITY,
                );
                Self::Forward {
                    camera: dunes_camera(),
                    light: DirectionalLight::default(),
                    renderer,
                }
            }
            Scene::Sprite => {
                let mut renderer = Box::new(SpriteRenderer::new(device, queue, format)?);
                let mut register = |label, width, height, pixels| {
                    renderer.register_sheet(
                        device,
                        &SheetDesc {
                            label,
                            width,
                            height,
                            // Pixel art's sampler, and the branch of
                            // `sprite.slang` a game actually ships on.
                            sample: SampleMode::Pixel,
                            pixels,
                        },
                    )
                };
                let sheets = match (
                    register("screenshot sheet A", 4, 2, &SPRITE_SHEET_A),
                    register("screenshot sheet B", 2, 2, &SPRITE_SHEET_B),
                ) {
                    (Ok(a), Ok(b)) => [a, b],
                    // Whichever failed, the renderer owns everything that did
                    // upload and gives it back here rather than at drop.
                    (Err(error), _) | (Ok(_), Err(error)) => {
                        renderer.destroy(device);
                        return Err(OffscreenError::Hal(error));
                    }
                };
                Self::Sprite { renderer, sheets }
            }
            Scene::Ao => {
                // The open box alone, and the cube parked out of frame — see
                // `ao_parked_cube`. Every other resident stays off for
                // `Scene::Spot`'s reason: what this frame is about is one
                // concave corner and the flat floor beside it.
                let mut renderer =
                    forward_renderer(device, queue, format, &crate::render::scene::demo(), path)?;
                place_cube(&mut renderer, ao_parked_cube());
                place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, ao_box());
                Self::Forward {
                    camera: ao_camera(),
                    light: ao_sun(),
                    renderer,
                }
            }
            Scene::Ssr => {
                // The whole of it is in `ssr_forward`, on `Scene::Aa`'s terms
                // below: `tests/render_e2e.rs` builds the same scene through
                // that function under three different skies, because what the
                // sky adds to a reflection is only recognisable against the
                // same frame without it.
                ssr_forward_on(
                    device,
                    queue,
                    format,
                    crcbl_render::Sky::NONE,
                    RenderEffects::DEFAULT_STACK,
                    &crate::render::scene::demo(),
                    DEMO_TINTED,
                    path,
                )?
                .into()
            }
            Scene::AtmosphereMirror => {
                // The floor, and nothing else in the frame — see the variant.
                // It is a plate of its own rather than the demo cube, for the
                // reason `atmosphere_mirror_mesh` gives: that cube's faces
                // carry vertex colours and a green mirror is not what this
                // fixture is predicting.
                let mut renderer =
                    forward_renderer(device, queue, format, &atmosphere_mirror_scene(), path)?;
                // **The reflection pair and nothing else.** Shadows have no
                // caster and no lit surface to fall on; the occlusion pass
                // scales an ambient term a conductor does not have; the
                // antialiasing resolve would filter the very gradient the
                // bands measure. Each of those is a term the host would have
                // to model to predict a floor pixel, and none of them is what
                // this frame is about.
                renderer.set_effect_request(EffectRequest {
                    camera: RenderEffects::REFLECTIONS,
                    ..EffectRequest::default()
                });
                renderer.set_atmosphere(Some(atmosphere_mirror_sky()));
                // Authored at its final size, so the instance carries the
                // identity: a plate whose corners are already world-space is
                // one fewer transform between the fixture and the host's own
                // prediction of a reflected ray.
                place(
                    &mut renderer,
                    ATMOSPHERE_MIRROR_MESH,
                    ATMOSPHERE_MIRROR_MATERIAL,
                    glam::Mat4::IDENTITY,
                );
                Self::Forward {
                    camera: atmosphere_mirror_camera(),
                    light: atmosphere_mirror_sun(),
                    renderer,
                }
            }
            Scene::GradientMirror => {
                // The arm above with `set_sky` in place of `set_atmosphere`,
                // and everything else shared: the same plate, the same camera,
                // the same effect request. What differs between the two
                // goldens is therefore which sky the reflection pass read, and
                // that is the whole of what this fixture is for.
                //
                // `atmosphere_mirror_sun` is reused because it contributes
                // nothing to either frame — its colour and its ambient are
                // both zero — so the direction it names is the only thing it
                // carries, and no pixel here can observe it.
                let mut renderer =
                    forward_renderer(device, queue, format, &atmosphere_mirror_scene(), path)?;
                renderer.set_effect_request(EffectRequest {
                    camera: RenderEffects::REFLECTIONS,
                    ..EffectRequest::default()
                });
                renderer.set_sky(gradient_mirror_sky());
                place(
                    &mut renderer,
                    ATMOSPHERE_MIRROR_MESH,
                    ATMOSPHERE_MIRROR_MATERIAL,
                    glam::Mat4::IDENTITY,
                );
                Self::Forward {
                    camera: atmosphere_mirror_camera(),
                    light: atmosphere_mirror_sun(),
                    renderer,
                }
            }
            Scene::StillPool => {
                // The whole of it is in `still_pool_forward`, on `Scene::Ssr`'s
                // terms: `tests/render_e2e.rs` builds the same scene with no
                // body, because the off-switch is only recognisable against it.
                still_pool::still_pool_forward_on_path(
                    device,
                    queue,
                    format,
                    &[still_pool_body()],
                    path,
                )?
                .into()
            }
            Scene::Meadow => {
                // The whole of it is in `meadow_forward`, on
                // `Scene::StillPool`'s terms: `tests/render_e2e.rs` builds the
                // same scene with no field, because the off-switch is only
                // recognisable against it.
                let field = meadow::meadow_field();
                meadow::meadow_forward_on_path(
                    device,
                    queue,
                    format,
                    Some(&field),
                    MeadowWind::Windy,
                    path,
                )?
                .into()
            }
            Scene::MeadowBlades => {
                // `Scene::Meadow`'s build with the blade meadow's field.
                let field = meadow::meadow_blades_field();
                meadow::meadow_forward_on_path(
                    device,
                    queue,
                    format,
                    Some(&field),
                    MeadowWind::Windy,
                    path,
                )?
                .into()
            }
            Scene::Occluders => {
                // The whole of it is in `occluders_forward_on_path`, on
                // `Scene::StillPool`'s terms: `tests/mesh_e2e/occlusion_cull.rs`
                // builds the same scene with the cull off and walks it along
                // its path.
                occluders::occluders_forward_on_path(
                    device,
                    queue,
                    format,
                    path,
                    OCCLUDERS_CULLING,
                )?
                .scene
                .into()
            }
            Scene::MeadowShells => {
                // `Scene::Meadow`'s build with the other look of its field.
                let field = meadow::meadow_shells_field();
                meadow::meadow_forward_on_path(
                    device,
                    queue,
                    format,
                    Some(&field),
                    MeadowWind::Windy,
                    path,
                )?
                .into()
            }
            Scene::Bloom => {
                // The floor every other overhead fixture stands on, and the
                // emitter laid on it — see `bloom_emitter`. Nothing else is in
                // frame, for `Scene::Spot`'s reason: what this frame is about is
                // one bright patch and the flat floor its halo spreads over.
                //
                // **The cube is placed as the floor rather than parked**, which
                // is why there is no `place_cube` call: it is still the first
                // insertion and still holds the pool slot every other scene
                // gives it.
                let mut renderer = forward_renderer(device, queue, format, &bloom_scene(), path)?;
                // **The one fixture that asks for the lens**, and the one that
                // asks for it from a file. `RenderEffects::DEFAULT_STACK` leaves
                // bloom out — a view that has declared no render stack has
                // declared no lens — so this is the camera-stack layer being
                // exercised as topic 18 describes it, RON and all, and it is
                // what keeps every other golden in the tree untouched.
                //
                // `set_camera_stack` moves that layer alone: the other three
                // stay where `ForwardRenderer::new` left them, which is where
                // the `EffectRequest::default()` this used to write put them.
                // See `BLOOM_STACK_RON` for what the stack says and why.
                renderer.set_camera_stack(&bloom_stack());
                place(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, spot_floor());
                place(&mut renderer, DEMO_CUBE, BLOOM_EMITTER, bloom_emitter());
                Self::Forward {
                    camera: bloom_camera(),
                    light: bloom_sun(),
                    renderer,
                }
            }
            Scene::Aa => {
                // The whole of it is in `aa_forward`, because
                // `tests/render_e2e.rs` builds the same scene through that
                // function with a different effect set — see its doc for why the
                // comparison cannot be made against a golden.
                aa_forward_on_path(device, queue, format, RenderEffects::DEFAULT_STACK, path)?
                    .into()
            }
            Scene::Probes => {
                // **The only scene here built from a description of its own**,
                // and the only thing that differs from `scene::demo`'s is the
                // probe grid — see `probe_scene`. The room is the open box and
                // nothing else: no cube, parked or otherwise, because a second
                // object standing on this floor is a second thing occluding the
                // bands that are the measurement.
                let mut renderer = forward_renderer(device, queue, format, &probe_scene(), path)?;
                // The fixture's measured pixels are diffuse probe irradiance.
                // Reflections now evaluate rough surfaces too, so refuse their
                // pair here rather than letting specular contaminate the Rust
                // mirror comparison below.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, probe_room());
                // The room is placed, so the probes can record it — see
                // `ForwardRenderer::capture_probe_visibility`, which is a call
                // rather than part of `with_scene` because a description has no
                // instances in it. Both probes stand in open air above this
                // floor and neither is behind anything, so what the capture buys
                // *here* is that the fixture exercises the read at all; the
                // leak it exists to stop is checked where geometry stands
                // between a probe and a surface.
                renderer.capture_probe_visibility(device, queue)?;
                Self::Forward {
                    camera: probe_camera(),
                    light: probe_sun(),
                    renderer,
                }
            }
            Scene::Ui => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::Widgets,
            },
            Scene::UiTree => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::Tree,
            },
            Scene::UiStyle => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::Style,
            },
            #[cfg(feature = "parsed-font")]
            Scene::UiText => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::Text,
            },
            #[cfg(not(feature = "parsed-font"))]
            Scene::UiText => {
                return Err(OffscreenError::Unusable(
                    "the ui_text scene draws the committed parsed font, which this build \
                     leaves out: turn on crcbl's `parsed-font` feature",
                ));
            }
            Scene::UiFocus => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::Focus,
            },
            Scene::UiWidgets => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::WidgetSet,
            },
            Scene::UiTextInput => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::TextInput,
            },
            Scene::UiLayout => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::Layout,
            },
            Scene::UiInspector => Self::Ui {
                renderer: ui_renderer(device, queue, format)?,
                atlas: FontAtlas::built_in(),
                content: UiContent::Inspector,
            },
            Scene::UiPrimitives => {
                let mut renderer = ui_renderer(device, queue, format)?;
                // Registered after the renderer uploaded its page, so the first
                // frame's `ui-images` pass is what puts them on the GPU.
                let images = match register_ui_primitives_images(renderer.images_mut()) {
                    Ok(images) => images,
                    Err(error) => {
                        renderer.destroy(device);
                        return Err(crate::hal::HalError::InvalidDescriptor(format!(
                            "the ui primitives scene's pictures: {error}"
                        ))
                        .into());
                    }
                };
                Self::Ui {
                    renderer,
                    atlas: FontAtlas::built_in(),
                    content: UiContent::Primitives(images),
                }
            }
        })
    }
}

// Keep renderer construction out of the dispatcher's frame. Otherwise debug
// builds reserve space for the unboxed renderer temporaries of every arm,
// even though only one scene is built.
fn forward_renderer(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    scene: &crate::render::scene::SceneDesc<'_>,
    path: crate::hal::GeometryPath,
) -> Result<Box<ForwardRenderer>, crate::hal::HalError> {
    ForwardRenderer::with_scene_on_path(device, queue, format, scene, path).map(Box::new)
}

fn ui_renderer(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
) -> Result<Box<UiRenderer>, crate::hal::HalError> {
    UiRenderer::new(device, queue, format).map(Box::new)
}
