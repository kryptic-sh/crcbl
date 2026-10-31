#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 7694 "hlsl.meta.slang"
matrix<float,int(3),int(3)>  abs_0(matrix<float,int(3),int(3)>  x_0)
{

#line 7572
    thread matrix<float,int(3),int(3)>  result_0;

#line 7572
    int i_0 = int(0);

#line 7572
    for(;;)
    {

#line 7572
        if(i_0 < int(3))
        {
        }
        else
        {

#line 7572
            break;
        }

#line 7572
        result_0[i_0] = abs(x_0[i_0]);

#line 7572
        i_0 = i_0 + int(1);

#line 7572
    }

#line 7572
    return result_0;
}


#line 476 "shaders/cull.slang"
bool reaches_0(float4 half_space_0, float3 center_0, float3 extent_0)
{
    float3 _S1 = half_space_0.xyz;
    return (dot(_S1, center_0) + half_space_0.w) >= (- dot(abs(_S1), extent_0));
}


#line 295
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 295
struct CullParams_natural_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_0;
    uint capacity_0;
    uint hidden_view_0;
    uint features_0;
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_view_proj_0;
    uint target_width_0;
    uint target_height_0;
    uint pyramid_levels_0;
    uint occlusion_pad_0;
    float small_feature_pixels_0;
    float small_feature_pad0_0;
    float small_feature_pad1_0;
    float small_feature_pad2_0;
    array<float4, int(24)> face_planes_0;
};


#line 344
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<packed_float4, int(4)> data_1;
};


#line 344
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_1 previous_transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint previous_base_vertex_0;
    float bounds_min_x_0;
    float bounds_min_y_0;
    float bounds_min_z_0;
    float bounds_max_x_0;
    float bounds_max_y_0;
    float bounds_max_z_0;
};


#line 177
struct GpuMesh_0
{
    uint base_vertex_1;
    uint base_index_0;
    uint index_count_0;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
    float uv_scale_u_0;
    float uv_scale_v_0;
    float uv_offset_u_0;
    float uv_offset_v_0;
    uint flags_1;
};


#line 414
struct KernelContext_0
{
    CullParams_natural_0 constant* cull_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    atomic<uint> device* visible_count_0;
    uint device* visible_0;
    depth2d<float, access::sample> pyramid_1_0;
    depth2d<float, access::sample> pyramid_2_0;
    depth2d<float, access::sample> pyramid_3_0;
    depth2d<float, access::sample> pyramid_4_0;
    depth2d<float, access::sample> pyramid_5_0;
    depth2d<float, access::sample> pyramid_6_0;
    depth2d<float, access::sample> pyramid_7_0;
    depth2d<float, access::sample> pyramid_8_0;
};


#line 483
bool in_frustum_0(float3 center_1, float3 extent_1, KernelContext_0 thread* kernelContext_0)
{

#line 483
    uint plane_0 = 0U;

    for(;;)
    {

#line 485
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 485
            break;
        }
        if(!reaches_0(kernelContext_0->cull_0->planes_0[plane_0], center_1, extent_1))
        {
            return false;
        }

#line 485
        plane_0 = plane_0 + 1U;

#line 485
    }

#line 492
    return true;
}


#line 2837 "core.meta.slang"
bool admits_0(uint _S2, KernelContext_0 thread* kernelContext_1)
{

#line 2837
    uint _S3 = (kernelContext_1->instances_0+_S2)->flags_0;

#line 426 "shaders/cull.slang"
    if((((kernelContext_1->instances_0+_S2)->flags_0) & 1U) == 0U)
    {
        return false;
    }

#line 434
    return (_S3 & (kernelContext_1->cull_0->hidden_view_0)) == 0U;
}


#line 434
bool has_box_0(uint _S4, KernelContext_0 thread* kernelContext_2)
{

#line 434
    uint _S5 = (kernelContext_2->instances_0+_S4)->flags_0;

#line 434
    bool _S6;

#line 445
    if((((kernelContext_2->instances_0+_S4)->flags_0) & 131072U) != 0U)
    {

#line 445
        _S6 = true;

#line 445
    }
    else
    {

#line 445
        _S6 = (_S5 & 2U) == 0U;

#line 445
    }

#line 444
    return _S6;
}


#line 444
void world_box_0(uint _S7, const GpuMesh_0 thread* _S8, float3 thread* _S9, float3 thread* _S10, KernelContext_0 thread* kernelContext_3)
{

#line 444
    GpuInstance_natural_0 device* _S11 = kernelContext_3->instances_0+_S7;

#line 457
    bool owns_0 = ((_S11->flags_0) & 131072U) != 0U;

#line 457
    float3 bounds_min_0;

    if(owns_0)
    {

#line 459
        bounds_min_0 = float3(_S11->bounds_min_x_0, _S11->bounds_min_y_0, _S11->bounds_min_z_0);

#line 459
    }
    else
    {

#line 459
        bounds_min_0 = float3(_S8->min_x_0, _S8->min_y_0, _S8->min_z_0);

#line 459
    }

#line 459
    float3 bounds_max_0;


    if(owns_0)
    {

#line 462
        bounds_max_0 = float3(_S11->bounds_max_x_0, _S11->bounds_max_y_0, _S11->bounds_max_z_0);

#line 462
    }
    else
    {

#line 462
        bounds_max_0 = float3(_S8->max_x_0, _S8->max_y_0, _S8->max_z_0);

#line 462
    }

#line 462
    float3 _S12 = float3(0.5f) ;


    float3 local_extent_0 = _S12 * (bounds_max_0 - bounds_min_0);

#line 465
    matrix<float,int(4),int(4)>  _S13 = matrix<float,int(4),int(4)> (_S11->transform_0.data_1[int(0)][int(0)], _S11->transform_0.data_1[int(1)][int(0)], _S11->transform_0.data_1[int(2)][int(0)], _S11->transform_0.data_1[int(3)][int(0)], _S11->transform_0.data_1[int(0)][int(1)], _S11->transform_0.data_1[int(1)][int(1)], _S11->transform_0.data_1[int(2)][int(1)], _S11->transform_0.data_1[int(3)][int(1)], _S11->transform_0.data_1[int(0)][int(2)], _S11->transform_0.data_1[int(1)][int(2)], _S11->transform_0.data_1[int(2)][int(2)], _S11->transform_0.data_1[int(3)][int(2)], _S11->transform_0.data_1[int(0)][int(3)], _S11->transform_0.data_1[int(1)][int(3)], _S11->transform_0.data_1[int(2)][int(3)], _S11->transform_0.data_1[int(3)][int(3)]);
    matrix<float,int(3),int(3)>  _S14 = matrix<float,int(3),int(3)> (_S13[int(0)].xyz, _S13[int(1)].xyz, _S13[int(2)].xyz);
    *_S9 = (((float4(_S12 * (bounds_max_0 + bounds_min_0), 1.0f)) * (_S13))).xyz;
    *_S10 = (((local_extent_0) * (abs_0(_S14))));
    return;
}


#line 661
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], CullParams_natural_0 constant* cull_1 [[buffer(0)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* visible_count_1 [[buffer(4)]], uint device* visible_1 [[buffer(3)]], depth2d<float, access::sample> pyramid_1_1 [[texture(0)]], depth2d<float, access::sample> pyramid_2_1 [[texture(1)]], depth2d<float, access::sample> pyramid_3_1 [[texture(2)]], depth2d<float, access::sample> pyramid_4_1 [[texture(3)]], depth2d<float, access::sample> pyramid_5_1 [[texture(4)]], depth2d<float, access::sample> pyramid_6_1 [[texture(5)]], depth2d<float, access::sample> pyramid_7_1 [[texture(6)]], depth2d<float, access::sample> pyramid_8_1 [[texture(7)]])
{

#line 661
    bool inside_0;

#line 661
    thread KernelContext_0 kernelContext_4;

#line 661
    (&kernelContext_4)->cull_0 = cull_1;

#line 661
    (&kernelContext_4)->instances_0 = instances_1;

#line 661
    (&kernelContext_4)->meshes_0 = meshes_1;

#line 661
    (&kernelContext_4)->visible_count_0 = visible_count_1;

#line 661
    (&kernelContext_4)->visible_0 = visible_1;

#line 661
    (&kernelContext_4)->pyramid_1_0 = pyramid_1_1;

#line 661
    (&kernelContext_4)->pyramid_2_0 = pyramid_2_1;

#line 661
    (&kernelContext_4)->pyramid_3_0 = pyramid_3_1;

#line 661
    (&kernelContext_4)->pyramid_4_0 = pyramid_4_1;

#line 661
    (&kernelContext_4)->pyramid_5_0 = pyramid_5_1;

#line 661
    (&kernelContext_4)->pyramid_6_0 = pyramid_6_1;

#line 661
    (&kernelContext_4)->pyramid_7_0 = pyramid_7_1;

#line 661
    (&kernelContext_4)->pyramid_8_0 = pyramid_8_1;

    uint index_0 = thread_0.x;
    if(index_0 >= (cull_1->instance_count_0))
    {
        return;
    }

#line 666
    GpuInstance_natural_0 device* _S15 = (&kernelContext_4)->instances_0+index_0;

#line 666
    bool _S16 = admits_0(index_0, &kernelContext_4);



    if(!_S16)
    {
        return;
    }
    GpuMesh_0 mesh_1 = (&kernelContext_4)->meshes_0[_S15->mesh_0];

#line 680
    if((mesh_1.index_count_0) == 0U)
    {
        return;
    }

#line 682
    bool _S17 = has_box_0(index_0, &kernelContext_4);

#line 682
    uint face_0;

#line 682
    uint entry_0;



    if(_S17)
    {

#line 686
        thread GpuMesh_0 _S18 = mesh_1;

        thread float3 center_2;
        thread float3 extent_2;

#line 689
        world_box_0(index_0, &_S18, &center_2, &extent_2, &kernelContext_4);

#line 689
        bool _S19 = in_frustum_0(center_2, extent_2, &kernelContext_4);

        if(!_S19)
        {
            return;
        }

#line 702
        if((((&kernelContext_4)->cull_0->features_0) & 1U) != 0U)
        {

#line 702
            face_0 = 0U;

#line 702
            entry_0 = index_0;

            for(;;)
            {

#line 704
                if(face_0 < 6U)
                {
                }
                else
                {

#line 704
                    break;
                }

#line 704
                uint plane_1 = 0U;


                for(;;)
                {

#line 707
                    if(plane_1 < 4U)
                    {
                    }
                    else
                    {

#line 707
                        inside_0 = true;

#line 707
                        break;
                    }
                    if(!reaches_0((&kernelContext_4)->cull_0->face_planes_0[face_0 * 4U + plane_1], center_2, extent_2))
                    {

#line 709
                        inside_0 = false;


                        break;
                    }

#line 707
                    plane_1 = plane_1 + 1U;

#line 707
                }

#line 715
                if(inside_0)
                {

#line 715
                    entry_0 = entry_0 | (1U << (24U + face_0));

#line 715
                }

#line 704
                face_0 = face_0 + 1U;

#line 704
            }

#line 702
        }
        else
        {

#line 702
            entry_0 = index_0;

#line 702
        }

#line 686
    }
    else
    {

#line 722
        if((((&kernelContext_4)->cull_0->features_0) & 1U) != 0U)
        {

#line 722
            face_0 = index_0 | 1056964608U;

#line 722
        }
        else
        {

#line 722
            face_0 = index_0;

#line 722
        }

#line 722
        entry_0 = face_0;

#line 686
    }

#line 730
    uint slot_0 = atomic_fetch_add_explicit((&kernelContext_4)->visible_count_0+0U, 1U, memory_order_relaxed);
    if(slot_0 < ((&kernelContext_4)->cull_0->capacity_0))
    {
        *((&kernelContext_4)->visible_0+slot_0) = entry_0;

#line 731
    }



    return;
}


#line 499
struct ScreenBounds_0
{
    bool valid_0;
    float min_x_1;
    float min_y_1;
    float max_x_1;
    float max_y_1;
    float nearest_0;
};


#line 518
ScreenBounds_0 project_box_0(matrix<float,int(4),int(4)>  view_proj_1, float3 center_3, float3 extent_3, float width_0, float height_0)
{
    thread ScreenBounds_0 bounds_0;
    (&bounds_0)->valid_0 = true;
    (&bounds_0)->min_x_1 = 1.6777216e+07f;
    (&bounds_0)->min_y_1 = 1.6777216e+07f;
    (&bounds_0)->max_x_1 = -1.6777216e+07f;
    (&bounds_0)->max_y_1 = -1.6777216e+07f;
    (&bounds_0)->nearest_0 = 0.0f;

#line 526
    uint corner_0 = 0U;
    for(;;)
    {

#line 527
        if(corner_0 < 8U)
        {
        }
        else
        {

#line 527
            break;
        }

#line 527
        float _S20;


        if((corner_0 & 1U) != 0U)
        {

#line 530
            _S20 = 1.0f;

#line 530
        }
        else
        {

#line 530
            _S20 = -1.0f;

#line 530
        }

#line 530
        float _S21;
        if((corner_0 & 2U) != 0U)
        {

#line 531
            _S21 = 1.0f;

#line 531
        }
        else
        {

#line 531
            _S21 = -1.0f;

#line 531
        }

#line 531
        float _S22;
        if((corner_0 & 4U) != 0U)
        {

#line 532
            _S22 = 1.0f;

#line 532
        }
        else
        {

#line 532
            _S22 = -1.0f;

#line 532
        }
        float4 clip_0 = (((float4(center_3 + float3(_S20, _S21, _S22) * extent_3, 1.0f)) * (view_proj_1)));


        float _S23 = clip_0.w;

#line 536
        if(!(_S23 > 0.0f))
        {
            (&bounds_0)->valid_0 = false;
            return bounds_0;
        }
        float depth_0 = clip_0.z / _S23;
        float x_1 = (clip_0.x / _S23 * 0.5f + 0.5f) * width_0;
        float y_0 = (0.5f - clip_0.y / _S23 * 0.5f) * height_0;

#line 543
        bool _S24;
        if(!(depth_0 <= 1.0f))
        {

#line 544
            _S24 = true;

#line 544
        }
        else
        {

#line 544
            _S24 = !((abs(x_1)) < 1.6777216e+07f);

#line 544
        }

#line 544
        bool _S25;

#line 544
        if(_S24)
        {

#line 544
            _S25 = true;

#line 544
        }
        else
        {

#line 544
            _S25 = !((abs(y_0)) < 1.6777216e+07f);

#line 544
        }

#line 544
        if(_S25)
        {
            (&bounds_0)->valid_0 = false;
            return bounds_0;
        }
        (&bounds_0)->min_x_1 = min((&bounds_0)->min_x_1, x_1);
        (&bounds_0)->min_y_1 = min((&bounds_0)->min_y_1, y_0);
        (&bounds_0)->max_x_1 = max((&bounds_0)->max_x_1, x_1);
        (&bounds_0)->max_y_1 = max((&bounds_0)->max_y_1, y_0);
        (&bounds_0)->nearest_0 = max((&bounds_0)->nearest_0, depth_0);

#line 527
        corner_0 = corner_0 + 1U;

#line 527
    }

#line 555
    return bounds_0;
}




float pyramid_load_0(uint level_0, int2 texel_0, KernelContext_0 thread* kernelContext_5)
{
    int3 at_0 = int3(texel_0, int(0));
    switch(level_0)
    {
    case 1U:
        {

#line 567
            return ((kernelContext_5->pyramid_1_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 2U:
        {

#line 569
            return ((kernelContext_5->pyramid_2_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 3U:
        {

#line 571
            return ((kernelContext_5->pyramid_3_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 4U:
        {

#line 573
            return ((kernelContext_5->pyramid_4_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 5U:
        {

#line 575
            return ((kernelContext_5->pyramid_5_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 6U:
        {

#line 577
            return ((kernelContext_5->pyramid_6_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 7U:
        {

#line 579
            return ((kernelContext_5->pyramid_7_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    default:
        {

#line 581
            return ((kernelContext_5->pyramid_8_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    }

#line 581
}


#line 601
bool occluded_0(matrix<float,int(4),int(4)>  view_proj_2, float3 center_4, float3 extent_4, KernelContext_0 thread* kernelContext_6)
{
    if((kernelContext_6->cull_0->pyramid_levels_0) == 0U)
    {
        return false;
    }
    ScreenBounds_0 bounds_1 = project_box_0(view_proj_2, center_4, extent_4, float(kernelContext_6->cull_0->target_width_0), float(kernelContext_6->cull_0->target_height_0));

    if(!bounds_1.valid_0)
    {
        return false;
    }
    int width_1 = int(kernelContext_6->cull_0->target_width_0);
    int height_1 = int(kernelContext_6->cull_0->target_height_0);
    int x0_0 = int(floor(bounds_1.min_x_1)) - int(1);
    int y0_0 = int(floor(bounds_1.min_y_1)) - int(1);
    int x1_0 = int(floor(bounds_1.max_x_1)) + int(1);
    int y1_0 = int(floor(bounds_1.max_y_1)) + int(1);

#line 618
    bool _S26;


    if(x1_0 < int(0))
    {

#line 621
        _S26 = true;

#line 621
    }
    else
    {

#line 621
        _S26 = y1_0 < int(0);

#line 621
    }

#line 621
    if(_S26)
    {

#line 621
        _S26 = true;

#line 621
    }
    else
    {

#line 621
        _S26 = x0_0 >= width_1;

#line 621
    }

#line 621
    if(_S26)
    {

#line 621
        _S26 = true;

#line 621
    }
    else
    {

#line 621
        _S26 = y0_0 >= height_1;

#line 621
    }

#line 621
    if(_S26)
    {
        return false;
    }
    uint left_0 = uint(max(x0_0, int(0)));
    uint top_0 = uint(max(y0_0, int(0)));
    uint right_0 = uint(min(x1_0, width_1 - int(1)));
    uint bottom_0 = uint(min(y1_0, height_1 - int(1)));

#line 628
    uint level_1 = 1U;


    for(;;)
    {

#line 632
        if(level_1 < (kernelContext_6->cull_0->pyramid_levels_0))
        {

#line 632
            if(((right_0 >> level_1) - (left_0 >> level_1)) > 1U)
            {

#line 632
                _S26 = true;

#line 632
            }
            else
            {

#line 632
                _S26 = ((bottom_0 >> level_1) - (top_0 >> level_1)) > 1U;

#line 632
            }

#line 632
        }
        else
        {

#line 632
            _S26 = false;

#line 632
        }

#line 631
        if(_S26)
        {
        }
        else
        {

#line 631
            break;
        }

#line 631
        level_1 = level_1 + 1U;

#line 631
    }

#line 638
    uint _S27 = max((kernelContext_6->cull_0->target_width_0) >> level_1, 1U) - 1U;

#line 638
    uint _S28 = min(left_0 >> level_1, _S27);
    uint _S29 = max((kernelContext_6->cull_0->target_height_0) >> level_1, 1U) - 1U;

#line 639
    uint _S30 = min(top_0 >> level_1, _S29);
    uint _S31 = min(right_0 >> level_1, _S27);
    uint _S32 = min(bottom_0 >> level_1, _S29);

#line 641
    float farthest_0 = 1.0f;

#line 641
    uint y_1 = _S30;



    for(;;)
    {

#line 645
        if(y_1 <= _S32)
        {
        }
        else
        {

#line 645
            break;
        }

#line 645
        uint x_2 = _S28;

        for(;;)
        {

#line 647
            if(x_2 <= _S31)
            {
            }
            else
            {

#line 647
                break;
            }

#line 647
            float _S33 = pyramid_load_0(level_1, int2(int(x_2), int(y_1)), kernelContext_6);

            float _S34 = min(farthest_0, _S33);

#line 647
            uint x_3 = x_2 + 1U;

#line 647
            farthest_0 = _S34;

#line 647
            x_2 = x_3;

#line 647
        }

#line 645
        y_1 = y_1 + 1U;

#line 645
    }

#line 652
    return (bounds_1.nearest_0 * 1.000244140625f) < farthest_0;
}


#line 750
[[kernel]] void occlusionMain(uint3 thread_1 [[thread_position_in_grid]], CullParams_natural_0 constant* cull_2 [[buffer(0)]], GpuInstance_natural_0 device* instances_2 [[buffer(1)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], atomic<uint> device* visible_count_2 [[buffer(4)]], uint device* visible_2 [[buffer(3)]], depth2d<float, access::sample> pyramid_1_2 [[texture(0)]], depth2d<float, access::sample> pyramid_2_2 [[texture(1)]], depth2d<float, access::sample> pyramid_3_2 [[texture(2)]], depth2d<float, access::sample> pyramid_4_2 [[texture(3)]], depth2d<float, access::sample> pyramid_5_2 [[texture(4)]], depth2d<float, access::sample> pyramid_6_2 [[texture(5)]], depth2d<float, access::sample> pyramid_7_2 [[texture(6)]], depth2d<float, access::sample> pyramid_8_2 [[texture(7)]])
{

#line 750
    thread KernelContext_0 kernelContext_7;

#line 750
    (&kernelContext_7)->cull_0 = cull_2;

#line 750
    (&kernelContext_7)->instances_0 = instances_2;

#line 750
    (&kernelContext_7)->meshes_0 = meshes_2;

#line 750
    (&kernelContext_7)->visible_count_0 = visible_count_2;

#line 750
    (&kernelContext_7)->visible_0 = visible_2;

#line 750
    (&kernelContext_7)->pyramid_1_0 = pyramid_1_2;

#line 750
    (&kernelContext_7)->pyramid_2_0 = pyramid_2_2;

#line 750
    (&kernelContext_7)->pyramid_3_0 = pyramid_3_2;

#line 750
    (&kernelContext_7)->pyramid_4_0 = pyramid_4_2;

#line 750
    (&kernelContext_7)->pyramid_5_0 = pyramid_5_2;

#line 750
    (&kernelContext_7)->pyramid_6_0 = pyramid_6_2;

#line 750
    (&kernelContext_7)->pyramid_7_0 = pyramid_7_2;

#line 750
    (&kernelContext_7)->pyramid_8_0 = pyramid_8_2;

    uint index_1 = thread_1.x;
    if(index_1 >= (cull_2->instance_count_0))
    {
        return;
    }

#line 755
    GpuInstance_natural_0 device* _S35 = (&kernelContext_7)->instances_0+index_1;

#line 755
    bool _S36 = admits_0(index_1, &kernelContext_7);



    if(!_S36)
    {
        return;
    }
    GpuMesh_0 mesh_2 = (&kernelContext_7)->meshes_0[_S35->mesh_0];

#line 769
    if((mesh_2.index_count_0) == 0U)
    {
        return;
    }

#line 771
    bool _S37 = has_box_0(index_1, &kernelContext_7);

#line 771
    uint entry_1;



    if(_S37)
    {

#line 775
        thread GpuMesh_0 _S38 = mesh_2;

        thread float3 center_5;
        thread float3 extent_5;

#line 778
        world_box_0(index_1, &_S38, &center_5, &extent_5, &kernelContext_7);

#line 778
        bool _S39 = in_frustum_0(center_5, extent_5, &kernelContext_7);

        if(!_S39)
        {
            return;
        }

#line 782
        bool _S40;

        if((((&kernelContext_7)->cull_0->features_0) & 4U) != 0U)
        {
            ScreenBounds_0 bounds_2 = project_box_0(matrix<float,int(4),int(4)> ((&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(3)]), center_5, extent_5, float((&kernelContext_7)->cull_0->target_width_0), float((&kernelContext_7)->cull_0->target_height_0));


            if(bounds_2.valid_0)
            {

#line 789
                _S40 = (max(bounds_2.max_x_1 - bounds_2.min_x_1, bounds_2.max_y_1 - bounds_2.min_y_1)) < ((&kernelContext_7)->cull_0->small_feature_pixels_0);

#line 789
            }
            else
            {

#line 789
                _S40 = false;

#line 789
            }

#line 788
            if(_S40)
            {


                uint _S41 = atomic_fetch_add_explicit((&kernelContext_7)->visible_count_0+7U, 1U, memory_order_relaxed);
                return;
            }

#line 784
        }

#line 797
        if((((&kernelContext_7)->cull_0->features_0) & 2U) != 0U)
        {

#line 797
            bool _S42 = occluded_0(matrix<float,int(4),int(4)> ((&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(0)][int(0)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(1)][int(0)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(2)][int(0)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(3)][int(0)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(0)][int(1)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(1)][int(1)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(2)][int(1)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(3)][int(1)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(0)][int(2)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(1)][int(2)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(2)][int(2)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(3)][int(2)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(0)][int(3)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(1)][int(3)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(2)][int(3)], (&kernelContext_7)->cull_0->previous_view_proj_0.data_0[int(3)][int(3)]), center_5, extent_5, &kernelContext_7);

#line 797
            _S40 = _S42;

#line 797
        }
        else
        {

#line 797
            _S40 = false;

#line 797
        }

#line 796
        if(_S40)
        {

            uint entry_2 = index_1 | 2147483648U;
            uint _S43 = atomic_fetch_add_explicit((&kernelContext_7)->visible_count_0+5U, 1U, memory_order_relaxed);

#line 800
            entry_1 = entry_2;

#line 796
        }
        else
        {

#line 796
            entry_1 = index_1;

#line 796
        }

#line 775
    }
    else
    {

#line 775
        entry_1 = index_1;

#line 775
    }

#line 804
    uint slot_1 = atomic_fetch_add_explicit((&kernelContext_7)->visible_count_0+0U, 1U, memory_order_relaxed);
    if(slot_1 < ((&kernelContext_7)->cull_0->capacity_0))
    {
        *((&kernelContext_7)->visible_0+slot_1) = entry_1;

#line 805
    }



    return;
}


#line 823
[[kernel]] void lateMain(uint3 thread_2 [[thread_position_in_grid]], CullParams_natural_0 constant* cull_3 [[buffer(0)]], GpuInstance_natural_0 device* instances_3 [[buffer(1)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], atomic<uint> device* visible_count_3 [[buffer(4)]], uint device* visible_3 [[buffer(3)]], depth2d<float, access::sample> pyramid_1_3 [[texture(0)]], depth2d<float, access::sample> pyramid_2_3 [[texture(1)]], depth2d<float, access::sample> pyramid_3_3 [[texture(2)]], depth2d<float, access::sample> pyramid_4_3 [[texture(3)]], depth2d<float, access::sample> pyramid_5_3 [[texture(4)]], depth2d<float, access::sample> pyramid_6_3 [[texture(5)]], depth2d<float, access::sample> pyramid_7_3 [[texture(6)]], depth2d<float, access::sample> pyramid_8_3 [[texture(7)]])
{

#line 823
    thread KernelContext_0 kernelContext_8;

#line 823
    (&kernelContext_8)->cull_0 = cull_3;

#line 823
    (&kernelContext_8)->instances_0 = instances_3;

#line 823
    (&kernelContext_8)->meshes_0 = meshes_3;

#line 823
    (&kernelContext_8)->visible_count_0 = visible_count_3;

#line 823
    (&kernelContext_8)->visible_0 = visible_3;

#line 823
    (&kernelContext_8)->pyramid_1_0 = pyramid_1_3;

#line 823
    (&kernelContext_8)->pyramid_2_0 = pyramid_2_3;

#line 823
    (&kernelContext_8)->pyramid_3_0 = pyramid_3_3;

#line 823
    (&kernelContext_8)->pyramid_4_0 = pyramid_4_3;

#line 823
    (&kernelContext_8)->pyramid_5_0 = pyramid_5_3;

#line 823
    (&kernelContext_8)->pyramid_6_0 = pyramid_6_3;

#line 823
    (&kernelContext_8)->pyramid_7_0 = pyramid_7_3;

#line 823
    (&kernelContext_8)->pyramid_8_0 = pyramid_8_3;

    uint index_2 = thread_2.x;
    uint _S44 = atomic_load_explicit(visible_count_3+0U, memory_order_relaxed);

#line 826
    if(index_2 >= (min(_S44, (&kernelContext_8)->cull_0->capacity_0)))
    {
        return;
    }
    uint device* _S45 = (&kernelContext_8)->visible_0+index_2;

#line 830
    uint entry_3 = *_S45;
    if(((*_S45) & 2147483648U) == 0U)
    {
        return;
    }
    uint _S46 = entry_3 & 16777215U;

#line 835
    thread GpuMesh_0 _S47 = (&kernelContext_8)->meshes_0[((&kernelContext_8)->instances_0+_S46)->mesh_0];

    thread float3 center_6;
    thread float3 extent_6;

#line 838
    world_box_0(_S46, &_S47, &center_6, &extent_6, &kernelContext_8);

#line 838
    bool _S48 = occluded_0(matrix<float,int(4),int(4)> ((&kernelContext_8)->cull_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_8)->cull_0->view_proj_0.data_0[int(3)][int(3)]), center_6, extent_6, &kernelContext_8);

    if(_S48)
    {
        uint _S49 = atomic_fetch_add_explicit((&kernelContext_8)->visible_count_0+6U, 1U, memory_order_relaxed);
        return;
    }
    *((&kernelContext_8)->visible_0+index_2) = entry_3 | 1073741824U;
    return;
}

