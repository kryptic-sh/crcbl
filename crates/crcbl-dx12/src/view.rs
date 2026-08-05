//! The four D3D12 view descriptors an [`ImageViewDesc`] can turn into.
//!
//! # One seam view is up to four D3D12 descriptors
//!
//! [`Device::create_image_view`](crcbl_hal::Device::create_image_view) makes one
//! object. D3D12 has no such object: it has a *shader resource view*, an
//! *unordered access view*, a *render target view* and a *depth stencil view*,
//! each a different struct written into a different heap type, and a texture
//! that is both sampled and rendered to genuinely needs two of them. So the
//! image's [`ImageUsage`](crcbl_hal::ImageUsage) decides which of these are
//! built, `device.rs` allocates a descriptor per built one, and the seam handle
//! names the set.
//!
//! # `None` is "D3D12 has no such view", and it is always a refusal
//!
//! Each function answers `None` for a combination D3D12 cannot express — a
//! depth stencil view of a volume, an unordered access view of a multisampled
//! texture, a cube view whose layer count is not a whole number of cubes.
//! `device.rs` turns every one of those into
//! [`HalError::InvalidDescriptor`](crcbl_hal::HalError::InvalidDescriptor)
//! naming it.
//!
//! **That refusal is the only diagnosis a caller will get.**
//! `ID3D12Device::CreateShaderResourceView` and its three siblings return
//! `void`: an invalid descriptor is a debug-layer message and a slot full of
//! nothing, with no error anywhere and a black or garbage sample at the far end.
//! Refusing before the write is what turns that into an `Err` at the call that
//! caused it.
//!
//! [`ImageViewDesc`]: crcbl_hal::ImageViewDesc

use crcbl_hal::ImageViewType;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING, D3D12_DEPTH_STENCIL_VIEW_DESC,
    D3D12_DEPTH_STENCIL_VIEW_DESC_0, D3D12_DSV_DIMENSION_TEXTURE1D, D3D12_DSV_DIMENSION_TEXTURE2D,
    D3D12_DSV_DIMENSION_TEXTURE2DARRAY, D3D12_DSV_DIMENSION_TEXTURE2DMS,
    D3D12_DSV_DIMENSION_TEXTURE2DMSARRAY, D3D12_DSV_FLAG_NONE, D3D12_RENDER_TARGET_VIEW_DESC,
    D3D12_RENDER_TARGET_VIEW_DESC_0, D3D12_RTV_DIMENSION_TEXTURE1D, D3D12_RTV_DIMENSION_TEXTURE2D,
    D3D12_RTV_DIMENSION_TEXTURE2DARRAY, D3D12_RTV_DIMENSION_TEXTURE2DMS,
    D3D12_RTV_DIMENSION_TEXTURE2DMSARRAY, D3D12_RTV_DIMENSION_TEXTURE3D,
    D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_SHADER_RESOURCE_VIEW_DESC_0,
    D3D12_SRV_DIMENSION_TEXTURE1D, D3D12_SRV_DIMENSION_TEXTURE2D,
    D3D12_SRV_DIMENSION_TEXTURE2DARRAY, D3D12_SRV_DIMENSION_TEXTURE2DMS,
    D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY, D3D12_SRV_DIMENSION_TEXTURE3D,
    D3D12_SRV_DIMENSION_TEXTURECUBE, D3D12_SRV_DIMENSION_TEXTURECUBEARRAY, D3D12_TEX1D_DSV,
    D3D12_TEX1D_RTV, D3D12_TEX1D_SRV, D3D12_TEX1D_UAV, D3D12_TEX2D_ARRAY_DSV,
    D3D12_TEX2D_ARRAY_RTV, D3D12_TEX2D_ARRAY_SRV, D3D12_TEX2D_ARRAY_UAV, D3D12_TEX2D_DSV,
    D3D12_TEX2D_RTV, D3D12_TEX2D_SRV, D3D12_TEX2D_UAV, D3D12_TEX2DMS_ARRAY_DSV,
    D3D12_TEX2DMS_ARRAY_RTV, D3D12_TEX2DMS_ARRAY_SRV, D3D12_TEX2DMS_DSV, D3D12_TEX2DMS_RTV,
    D3D12_TEX2DMS_SRV, D3D12_TEX3D_RTV, D3D12_TEX3D_SRV, D3D12_TEX3D_UAV, D3D12_TEXCUBE_ARRAY_SRV,
    D3D12_TEXCUBE_SRV, D3D12_UAV_DIMENSION_TEXTURE1D, D3D12_UAV_DIMENSION_TEXTURE2D,
    D3D12_UAV_DIMENSION_TEXTURE2DARRAY, D3D12_UAV_DIMENSION_TEXTURE3D,
    D3D12_UNORDERED_ACCESS_VIEW_DESC, D3D12_UNORDERED_ACCESS_VIEW_DESC_0,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

/// Layers in one cube map. Fixed by the shape of a cube, not by any API.
const FACES_PER_CUBE: u32 = 6;

/// The part of an image a view covers, with the seam's "all remaining"
/// sentinels already resolved against the image's real extent.
///
/// Resolved before it gets here on purpose: D3D12's view structs take absolute
/// counts and would read
/// [`ImageSubresourceRange::ALL`](crcbl_hal::ImageSubresourceRange::ALL) as a
/// literal four billion levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Subresource {
    /// First mip level. A render target or depth stencil view addresses exactly
    /// this one — D3D12 has no multi-mip attachment.
    pub(crate) base_mip: u32,
    /// Mip levels the view samples through.
    pub(crate) mip_count: u32,
    /// First array layer, or first depth slice for a volume.
    pub(crate) base_layer: u32,
    /// Array layers, or depth slices for a volume.
    pub(crate) layer_count: u32,
    /// Samples per texel of the image being viewed.
    pub(crate) samples: u32,
}

impl Subresource {
    const fn multisampled(self) -> bool {
        self.samples > 1
    }
}

/// A shader resource view — what a sampled texture is bound through.
pub(crate) fn shader_resource(
    format: DXGI_FORMAT,
    view: ImageViewType,
    sub: Subresource,
) -> Option<D3D12_SHADER_RESOURCE_VIEW_DESC> {
    let (dimension, anonymous) = match view {
        ImageViewType::D1 => (
            D3D12_SRV_DIMENSION_TEXTURE1D,
            D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture1D: D3D12_TEX1D_SRV {
                    MostDetailedMip: sub.base_mip,
                    MipLevels: sub.mip_count,
                    ResourceMinLODClamp: 0.0,
                },
            },
        ),
        ImageViewType::D2 if sub.multisampled() => (
            D3D12_SRV_DIMENSION_TEXTURE2DMS,
            D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DMS: D3D12_TEX2DMS_SRV {
                    UnusedField_NothingToDefine: 0,
                },
            },
        ),
        ImageViewType::D2 => (
            D3D12_SRV_DIMENSION_TEXTURE2D,
            D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: sub.base_mip,
                    MipLevels: sub.mip_count,
                    // Plane zero is the depth plane of a depth/stencil format
                    // and the only plane of everything else; see
                    // `conv::depth_read_format` for why stencil is not
                    // reachable through this seam.
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        ),
        ImageViewType::D2Array if sub.multisampled() => (
            D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY,
            D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DMSArray: D3D12_TEX2DMS_ARRAY_SRV {
                    FirstArraySlice: sub.base_layer,
                    ArraySize: sub.layer_count,
                },
            },
        ),
        ImageViewType::D2Array => (
            D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
            D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                    MostDetailedMip: sub.base_mip,
                    MipLevels: sub.mip_count,
                    FirstArraySlice: sub.base_layer,
                    ArraySize: sub.layer_count,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        ),
        ImageViewType::Cube => {
            if sub.layer_count != FACES_PER_CUBE || sub.multisampled() {
                return None;
            }
            (
                D3D12_SRV_DIMENSION_TEXTURECUBE,
                D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    TextureCube: D3D12_TEXCUBE_SRV {
                        MostDetailedMip: sub.base_mip,
                        MipLevels: sub.mip_count,
                        ResourceMinLODClamp: 0.0,
                    },
                },
            )
        }
        ImageViewType::CubeArray => {
            if sub.layer_count == 0
                || !sub.layer_count.is_multiple_of(FACES_PER_CUBE)
                || sub.multisampled()
            {
                return None;
            }
            (
                D3D12_SRV_DIMENSION_TEXTURECUBEARRAY,
                D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    TextureCubeArray: D3D12_TEXCUBE_ARRAY_SRV {
                        MostDetailedMip: sub.base_mip,
                        MipLevels: sub.mip_count,
                        First2DArrayFace: sub.base_layer,
                        NumCubes: sub.layer_count / FACES_PER_CUBE,
                        ResourceMinLODClamp: 0.0,
                    },
                },
            )
        }
        ImageViewType::D3 => {
            if sub.multisampled() {
                return None;
            }
            (
                D3D12_SRV_DIMENSION_TEXTURE3D,
                D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture3D: D3D12_TEX3D_SRV {
                        MostDetailedMip: sub.base_mip,
                        MipLevels: sub.mip_count,
                        ResourceMinLODClamp: 0.0,
                    },
                },
            )
        }
    };
    Some(D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: dimension,
        // **Not zero.** `Shader4ComponentMapping` is a packed swizzle and the
        // all-zero value maps every channel onto red, which is a texture that
        // samples as a grey ramp with no error anywhere. The identity swizzle
        // has a name, and this is it.
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: anonymous,
    })
}

/// An unordered access view — what a storage image is bound through.
///
/// `None` for a multisampled image: D3D12 has a `TEXTURE2DMS` UAV dimension, but
/// writing through it needs the optional `WriteableMSAATextures` capability,
/// which nothing in this backend queries. A cube view becomes a plain array,
/// which is what D3D12 offers — there is no cube UAV, and the faces are the
/// layers.
pub(crate) fn unordered_access(
    format: DXGI_FORMAT,
    view: ImageViewType,
    sub: Subresource,
) -> Option<D3D12_UNORDERED_ACCESS_VIEW_DESC> {
    if sub.multisampled() {
        return None;
    }
    let (dimension, anonymous) = match view {
        ImageViewType::D1 => (
            D3D12_UAV_DIMENSION_TEXTURE1D,
            D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                Texture1D: D3D12_TEX1D_UAV {
                    MipSlice: sub.base_mip,
                },
            },
        ),
        ImageViewType::D2 => (
            D3D12_UAV_DIMENSION_TEXTURE2D,
            D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_UAV {
                    MipSlice: sub.base_mip,
                    PlaneSlice: 0,
                },
            },
        ),
        ImageViewType::D2Array | ImageViewType::Cube | ImageViewType::CubeArray => (
            D3D12_UAV_DIMENSION_TEXTURE2DARRAY,
            D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                Texture2DArray: D3D12_TEX2D_ARRAY_UAV {
                    MipSlice: sub.base_mip,
                    FirstArraySlice: sub.base_layer,
                    ArraySize: sub.layer_count,
                    PlaneSlice: 0,
                },
            },
        ),
        ImageViewType::D3 => (
            D3D12_UAV_DIMENSION_TEXTURE3D,
            D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                Texture3D: D3D12_TEX3D_UAV {
                    MipSlice: sub.base_mip,
                    FirstWSlice: sub.base_layer,
                    WSize: sub.layer_count,
                },
            },
        ),
    };
    Some(D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: format,
        ViewDimension: dimension,
        Anonymous: anonymous,
    })
}

/// A render target view.
///
/// A cube view becomes a plain 2D array: D3D12 has no cube render target, and
/// rendering to a face is rendering to the layer it is.
pub(crate) fn render_target(
    format: DXGI_FORMAT,
    view: ImageViewType,
    sub: Subresource,
) -> Option<D3D12_RENDER_TARGET_VIEW_DESC> {
    let (dimension, anonymous) = match view {
        ImageViewType::D1 => (
            D3D12_RTV_DIMENSION_TEXTURE1D,
            D3D12_RENDER_TARGET_VIEW_DESC_0 {
                Texture1D: D3D12_TEX1D_RTV {
                    MipSlice: sub.base_mip,
                },
            },
        ),
        ImageViewType::D2 if sub.multisampled() => (
            D3D12_RTV_DIMENSION_TEXTURE2DMS,
            D3D12_RENDER_TARGET_VIEW_DESC_0 {
                Texture2DMS: D3D12_TEX2DMS_RTV {
                    UnusedField_NothingToDefine: 0,
                },
            },
        ),
        ImageViewType::D2 => (
            D3D12_RTV_DIMENSION_TEXTURE2D,
            D3D12_RENDER_TARGET_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_RTV {
                    MipSlice: sub.base_mip,
                    PlaneSlice: 0,
                },
            },
        ),
        ImageViewType::D2Array | ImageViewType::Cube | ImageViewType::CubeArray
            if sub.multisampled() =>
        {
            (
                D3D12_RTV_DIMENSION_TEXTURE2DMSARRAY,
                D3D12_RENDER_TARGET_VIEW_DESC_0 {
                    Texture2DMSArray: D3D12_TEX2DMS_ARRAY_RTV {
                        FirstArraySlice: sub.base_layer,
                        ArraySize: sub.layer_count,
                    },
                },
            )
        }
        ImageViewType::D2Array | ImageViewType::Cube | ImageViewType::CubeArray => (
            D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
            D3D12_RENDER_TARGET_VIEW_DESC_0 {
                Texture2DArray: D3D12_TEX2D_ARRAY_RTV {
                    MipSlice: sub.base_mip,
                    FirstArraySlice: sub.base_layer,
                    ArraySize: sub.layer_count,
                    PlaneSlice: 0,
                },
            },
        ),
        ImageViewType::D3 => {
            if sub.multisampled() {
                return None;
            }
            (
                D3D12_RTV_DIMENSION_TEXTURE3D,
                D3D12_RENDER_TARGET_VIEW_DESC_0 {
                    Texture3D: D3D12_TEX3D_RTV {
                        MipSlice: sub.base_mip,
                        FirstWSlice: sub.base_layer,
                        WSize: sub.layer_count,
                    },
                },
            )
        }
    };
    Some(D3D12_RENDER_TARGET_VIEW_DESC {
        Format: format,
        ViewDimension: dimension,
        Anonymous: anonymous,
    })
}

/// A depth stencil view.
///
/// `None` for a volume: D3D12's `D3D12_DSV_DIMENSION` has no 3D member at all,
/// because depth testing runs against a 2D attachment.
///
/// [`D3D12_DSV_FLAG_NONE`] is always the flag set. The two flags that exist —
/// `READ_ONLY_DEPTH` and `READ_ONLY_STENCIL` — declare that a pass will sample
/// the attachment while it is bound, and the seam has no vocabulary for that
/// intent; guessing it from [`ImageUsage`](crcbl_hal::ImageUsage) would make
/// every sampled depth target read-only and silently stop it being written.
pub(crate) fn depth_stencil(
    format: DXGI_FORMAT,
    view: ImageViewType,
    sub: Subresource,
) -> Option<D3D12_DEPTH_STENCIL_VIEW_DESC> {
    let (dimension, anonymous) = match view {
        ImageViewType::D1 => (
            D3D12_DSV_DIMENSION_TEXTURE1D,
            D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture1D: D3D12_TEX1D_DSV {
                    MipSlice: sub.base_mip,
                },
            },
        ),
        ImageViewType::D2 if sub.multisampled() => (
            D3D12_DSV_DIMENSION_TEXTURE2DMS,
            D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2DMS: D3D12_TEX2DMS_DSV {
                    UnusedField_NothingToDefine: 0,
                },
            },
        ),
        ImageViewType::D2 => (
            D3D12_DSV_DIMENSION_TEXTURE2D,
            D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_DSV {
                    MipSlice: sub.base_mip,
                },
            },
        ),
        ImageViewType::D2Array | ImageViewType::Cube | ImageViewType::CubeArray
            if sub.multisampled() =>
        {
            (
                D3D12_DSV_DIMENSION_TEXTURE2DMSARRAY,
                D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                    Texture2DMSArray: D3D12_TEX2DMS_ARRAY_DSV {
                        FirstArraySlice: sub.base_layer,
                        ArraySize: sub.layer_count,
                    },
                },
            )
        }
        ImageViewType::D2Array | ImageViewType::Cube | ImageViewType::CubeArray => (
            D3D12_DSV_DIMENSION_TEXTURE2DARRAY,
            D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2DArray: D3D12_TEX2D_ARRAY_DSV {
                    MipSlice: sub.base_mip,
                    FirstArraySlice: sub.base_layer,
                    ArraySize: sub.layer_count,
                },
            },
        ),
        ImageViewType::D3 => return None,
    };
    Some(D3D12_DEPTH_STENCIL_VIEW_DESC {
        Format: format,
        ViewDimension: dimension,
        Flags: D3D12_DSV_FLAG_NONE,
        Anonymous: anonymous,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT,
    };

    /// Every view type the seam declares, so the properties below cover all of
    /// them rather than the two that were convenient.
    const VIEWS: &[ImageViewType] = &[
        ImageViewType::D1,
        ImageViewType::D2,
        ImageViewType::D2Array,
        ImageViewType::Cube,
        ImageViewType::CubeArray,
        ImageViewType::D3,
    ];

    /// A single-layer, single-sample, whole-chain subresource.
    const fn flat(mip_count: u32) -> Subresource {
        Subresource {
            base_mip: 0,
            mip_count,
            base_layer: 0,
            layer_count: 1,
            samples: 1,
        }
    }

    /// A cube's six layers.
    const fn cube(cubes: u32) -> Subresource {
        Subresource {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: cubes * FACES_PER_CUBE,
            samples: 1,
        }
    }

    /// Every view type reaches its **own** SRV dimension, and none collapses
    /// onto another.
    ///
    /// The duplicate check is what a transposed arm trips. A cube view that
    /// produced `TEXTURE2DARRAY` still writes a valid descriptor and still
    /// samples — it samples the wrong thing, with a `float3` direction read as
    /// a layer index, and nothing returns an error.
    #[test]
    fn every_view_type_has_its_own_shader_resource_dimension() {
        assert!(!VIEWS.is_empty(), "nothing to check");
        let mut seen = Vec::new();
        for &view in VIEWS {
            let sub = if matches!(view, ImageViewType::Cube | ImageViewType::CubeArray) {
                cube(1)
            } else {
                flat(1)
            };
            let desc = shader_resource(DXGI_FORMAT_R16G16B16A16_FLOAT, view, sub)
                .unwrap_or_else(|| panic!("{view:?} has no shader resource view"));
            assert!(
                !seen.contains(&desc.ViewDimension),
                "{view:?} duplicates dimension {:?}",
                desc.ViewDimension
            );
            seen.push(desc.ViewDimension);
            assert_eq!(
                desc.Shader4ComponentMapping, D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                "{view:?}: an all-zero swizzle samples every channel as red"
            );
            assert_eq!(desc.Format, DXGI_FORMAT_R16G16B16A16_FLOAT, "{view:?}");
        }
        assert_eq!(seen.len(), VIEWS.len());
    }

    /// A cube view is only a cube when its layers are whole cubes.
    ///
    /// The falsifying value is a rule that divides and rounds: five layers is
    /// not a cube and eight is not a cube array, and a view that accepted
    /// either would index past the last face.
    #[test]
    fn cube_views_need_a_whole_number_of_cubes() {
        let one = cube(1);
        let desc = shader_resource(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::Cube, one)
            .expect("six layers are one cube");
        assert_eq!(desc.ViewDimension, D3D12_SRV_DIMENSION_TEXTURECUBE);

        let three = cube(3);
        let desc = shader_resource(
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            ImageViewType::CubeArray,
            three,
        )
        .expect("eighteen layers are three cubes");
        assert_eq!(desc.ViewDimension, D3D12_SRV_DIMENSION_TEXTURECUBEARRAY);
        // SAFETY: the arm above set `TextureCubeArray`, which is the union
        // member this dimension names and the one just asserted on.
        let cubes = unsafe { desc.Anonymous.TextureCubeArray.NumCubes };
        assert_eq!(cubes, 3, "the layer count did not become a cube count");

        // None of these is a whole number of cubes, so both view types must
        // refuse every one of them.
        for bad in [1u32, 5, 7, 8] {
            assert!(
                !bad.is_multiple_of(FACES_PER_CUBE),
                "{bad} is a whole cube after all"
            );
            let sub = Subresource {
                layer_count: bad,
                ..one
            };
            assert!(
                shader_resource(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::Cube, sub).is_none(),
                "{bad} layers accepted as a cube"
            );
            assert!(
                shader_resource(
                    DXGI_FORMAT_R16G16B16A16_FLOAT,
                    ImageViewType::CubeArray,
                    sub
                )
                .is_none(),
                "{bad} layers accepted as a cube array"
            );
        }
    }

    /// The combinations D3D12 has no member for are refused rather than
    /// approximated.
    #[test]
    fn the_views_d3d12_does_not_have_are_refused() {
        let volume = flat(1);
        assert!(
            depth_stencil(DXGI_FORMAT_D32_FLOAT, ImageViewType::D3, volume).is_none(),
            "D3D12_DSV_DIMENSION has no 3D member"
        );
        let multisampled = Subresource {
            samples: 4,
            ..flat(1)
        };
        assert!(
            unordered_access(
                DXGI_FORMAT_R16G16B16A16_FLOAT,
                ImageViewType::D2,
                multisampled
            )
            .is_none(),
            "a multisampled UAV needs a capability this backend does not query"
        );
        assert!(
            shader_resource(
                DXGI_FORMAT_R16G16B16A16_FLOAT,
                ImageViewType::Cube,
                Subresource {
                    samples: 4,
                    ..cube(1)
                }
            )
            .is_none(),
            "there is no multisampled cube"
        );
        // And the ones D3D12 does have are still built, so the refusals above
        // are about the combination and not about the function.
        assert!(
            depth_stencil(DXGI_FORMAT_D32_FLOAT, ImageViewType::D2, multisampled).is_some(),
            "a multisampled depth attachment is ordinary"
        );
        assert!(
            unordered_access(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::D3, volume).is_some()
        );
    }

    /// A multisampled 2D view takes the `MS` dimension in all three of the
    /// descriptor kinds that have one.
    ///
    /// The sample count is the key here and is the only field that varies: a
    /// backend that ignored it would produce a plain `TEXTURE2D` view of a
    /// multisampled resource, which D3D12 rejects at write time with no return
    /// value to say so.
    #[test]
    fn multisampling_selects_the_ms_dimensions() {
        let sub = Subresource {
            samples: 4,
            ..flat(1)
        };
        let single = flat(1);

        let srv = shader_resource(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::D2, sub)
            .expect("a multisampled SRV");
        assert_eq!(srv.ViewDimension, D3D12_SRV_DIMENSION_TEXTURE2DMS);
        assert_eq!(
            shader_resource(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::D2, single)
                .expect("a single-sampled SRV")
                .ViewDimension,
            D3D12_SRV_DIMENSION_TEXTURE2D
        );

        let rtv = render_target(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::D2, sub)
            .expect("a multisampled RTV");
        assert_eq!(rtv.ViewDimension, D3D12_RTV_DIMENSION_TEXTURE2DMS);

        let dsv = depth_stencil(DXGI_FORMAT_D32_FLOAT, ImageViewType::D2Array, sub)
            .expect("a multisampled array DSV");
        assert_eq!(dsv.ViewDimension, D3D12_DSV_DIMENSION_TEXTURE2DMSARRAY);
    }

    /// An attachment addresses one mip and the requested layers, and the
    /// subresource fields really arrive in the descriptor.
    ///
    /// Reading the union back is the point: every field defaults to zero, so a
    /// descriptor built from the wrong member — or from a subresource nobody
    /// copied in — is all zeros and names mip 0 layer 0, which is a plausible
    /// view of the wrong part of the image.
    #[test]
    fn an_attachment_view_carries_the_mip_and_layers_it_was_given() {
        let sub = Subresource {
            base_mip: 2,
            mip_count: 1,
            base_layer: 3,
            layer_count: 4,
            samples: 1,
        };
        let rtv = render_target(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::D2Array, sub)
            .expect("an array RTV");
        assert_eq!(rtv.ViewDimension, D3D12_RTV_DIMENSION_TEXTURE2DARRAY);
        // SAFETY: the dimension just asserted on is the one whose arm set
        // `Texture2DArray`, so that is the live union member.
        let array = unsafe { rtv.Anonymous.Texture2DArray };
        assert_eq!(array.MipSlice, 2, "the base mip was dropped");
        assert_eq!(array.FirstArraySlice, 3, "the base layer was dropped");
        assert_eq!(array.ArraySize, 4, "the layer count was dropped");

        let srv = shader_resource(DXGI_FORMAT_R16G16B16A16_FLOAT, ImageViewType::D2, flat(7))
            .expect("a 2D SRV");
        // SAFETY: `D3D12_SRV_DIMENSION_TEXTURE2D` is what the arm above
        // produced, and `Texture2D` is the member it wrote.
        let texture = unsafe { srv.Anonymous.Texture2D };
        assert_eq!(texture.MipLevels, 7, "the mip count was dropped");
        assert_eq!(texture.MostDetailedMip, 0);
    }
}
