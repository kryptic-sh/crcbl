#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 7508 "hlsl.meta.slang"
matrix<float,int(3),int(3)>  abs_0(matrix<float,int(3),int(3)>  x_0)
{

#line 7386
    thread matrix<float,int(3),int(3)>  result_0;

#line 7386
    int i_0 = int(0);

#line 7386
    for(;;)
    {

#line 7386
        if(i_0 < int(3))
        {
        }
        else
        {

#line 7386
            break;
        }

#line 7386
        result_0[i_0] = abs(x_0[i_0]);

#line 7386
        i_0 = i_0 + int(1);

#line 7386
    }

#line 7386
    return result_0;
}


#line 447 "shaders/cull.slang"
bool reaches_0(float4 half_space_0, float3 center_0, float3 extent_0)
{
    float3 _S1 = half_space_0.xyz;
    return (dot(_S1, center_0) + half_space_0.w) >= (- dot(abs(_S1), extent_0));
}


#line 284
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 284
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


#line 284
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<packed_float4, int(4)> data_1;
};


#line 284
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
    uint pad1_0;
    uint pad2_0;
};


#line 166
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


#line 703
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


#line 454
bool in_frustum_0(float3 center_1, float3 extent_1, KernelContext_0 thread* kernelContext_0)
{

#line 454
    uint plane_0 = 0U;

    for(;;)
    {

#line 456
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 456
            break;
        }
        if(!reaches_0(kernelContext_0->cull_0->planes_0[plane_0], center_1, extent_1))
        {
            return false;
        }

#line 456
        plane_0 = plane_0 + 1U;

#line 456
    }

#line 463
    return true;
}


#line 2819 "core.meta.slang"
bool admits_0(uint _S2, KernelContext_0 thread* kernelContext_1)
{

#line 2819
    uint _S3 = (kernelContext_1->instances_0+_S2)->flags_0;

#line 415 "shaders/cull.slang"
    if((((kernelContext_1->instances_0+_S2)->flags_0) & 1U) == 0U)
    {
        return false;
    }

#line 423
    return (_S3 & (kernelContext_1->cull_0->hidden_view_0)) == 0U;
}


#line 423
void world_box_0(uint _S4, const GpuMesh_0 thread* _S5, float3 thread* _S6, float3 thread* _S7, KernelContext_0 thread* kernelContext_2)
{

#line 433
    float3 bounds_min_0 = float3(_S5->min_x_0, _S5->min_y_0, _S5->min_z_0);
    float3 bounds_max_0 = float3(_S5->max_x_0, _S5->max_y_0, _S5->max_z_0);

#line 434
    float3 _S8 = float3(0.5f) ;

    float3 local_extent_0 = _S8 * (bounds_max_0 - bounds_min_0);

#line 436
    matrix<float,int(4),int(4)>  _S9 = matrix<float,int(4),int(4)> ((kernelContext_2->instances_0+_S4)->transform_0.data_1[int(0)][int(0)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(1)][int(0)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(2)][int(0)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(3)][int(0)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(0)][int(1)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(1)][int(1)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(2)][int(1)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(3)][int(1)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(0)][int(2)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(1)][int(2)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(2)][int(2)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(3)][int(2)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(0)][int(3)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(1)][int(3)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(2)][int(3)], (kernelContext_2->instances_0+_S4)->transform_0.data_1[int(3)][int(3)]);
    matrix<float,int(3),int(3)>  _S10 = matrix<float,int(3),int(3)> (_S9[int(0)].xyz, _S9[int(1)].xyz, _S9[int(2)].xyz);
    *_S6 = (((float4(_S8 * (bounds_max_0 + bounds_min_0), 1.0f)) * (_S9))).xyz;
    *_S7 = (((local_extent_0) * (abs_0(_S10))));
    return;
}


#line 632
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], CullParams_natural_0 constant* cull_1 [[buffer(0)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* visible_count_1 [[buffer(4)]], uint device* visible_1 [[buffer(3)]], depth2d<float, access::sample> pyramid_1_1 [[texture(0)]], depth2d<float, access::sample> pyramid_2_1 [[texture(1)]], depth2d<float, access::sample> pyramid_3_1 [[texture(2)]], depth2d<float, access::sample> pyramid_4_1 [[texture(3)]], depth2d<float, access::sample> pyramid_5_1 [[texture(4)]], depth2d<float, access::sample> pyramid_6_1 [[texture(5)]], depth2d<float, access::sample> pyramid_7_1 [[texture(6)]], depth2d<float, access::sample> pyramid_8_1 [[texture(7)]])
{

#line 632
    bool inside_0;

#line 632
    thread KernelContext_0 kernelContext_3;

#line 632
    (&kernelContext_3)->cull_0 = cull_1;

#line 632
    (&kernelContext_3)->instances_0 = instances_1;

#line 632
    (&kernelContext_3)->meshes_0 = meshes_1;

#line 632
    (&kernelContext_3)->visible_count_0 = visible_count_1;

#line 632
    (&kernelContext_3)->visible_0 = visible_1;

#line 632
    (&kernelContext_3)->pyramid_1_0 = pyramid_1_1;

#line 632
    (&kernelContext_3)->pyramid_2_0 = pyramid_2_1;

#line 632
    (&kernelContext_3)->pyramid_3_0 = pyramid_3_1;

#line 632
    (&kernelContext_3)->pyramid_4_0 = pyramid_4_1;

#line 632
    (&kernelContext_3)->pyramid_5_0 = pyramid_5_1;

#line 632
    (&kernelContext_3)->pyramid_6_0 = pyramid_6_1;

#line 632
    (&kernelContext_3)->pyramid_7_0 = pyramid_7_1;

#line 632
    (&kernelContext_3)->pyramid_8_0 = pyramid_8_1;

    uint index_0 = thread_0.x;
    if(index_0 >= (cull_1->instance_count_0))
    {
        return;
    }

#line 637
    GpuInstance_natural_0 device* _S11 = (&kernelContext_3)->instances_0+index_0;

#line 637
    bool _S12 = admits_0(index_0, &kernelContext_3);



    if(!_S12)
    {
        return;
    }
    GpuMesh_0 mesh_1 = (&kernelContext_3)->meshes_0[_S11->mesh_0];

#line 651
    if((mesh_1.index_count_0) == 0U)
    {
        return;
    }

#line 653
    uint face_0;

#line 653
    uint entry_0;



    if(((_S11->flags_0) & 2U) == 0U)
    {

#line 657
        thread GpuMesh_0 _S13 = mesh_1;

        thread float3 center_2;
        thread float3 extent_2;

#line 660
        world_box_0(index_0, &_S13, &center_2, &extent_2, &kernelContext_3);

#line 660
        bool _S14 = in_frustum_0(center_2, extent_2, &kernelContext_3);

        if(!_S14)
        {
            return;
        }

#line 673
        if((((&kernelContext_3)->cull_0->features_0) & 1U) != 0U)
        {

#line 673
            face_0 = 0U;

#line 673
            entry_0 = index_0;

            for(;;)
            {

#line 675
                if(face_0 < 6U)
                {
                }
                else
                {

#line 675
                    break;
                }

#line 675
                uint plane_1 = 0U;


                for(;;)
                {

#line 678
                    if(plane_1 < 4U)
                    {
                    }
                    else
                    {

#line 678
                        inside_0 = true;

#line 678
                        break;
                    }
                    if(!reaches_0((&kernelContext_3)->cull_0->face_planes_0[face_0 * 4U + plane_1], center_2, extent_2))
                    {

#line 680
                        inside_0 = false;


                        break;
                    }

#line 678
                    plane_1 = plane_1 + 1U;

#line 678
                }

#line 686
                if(inside_0)
                {

#line 686
                    entry_0 = entry_0 | (1U << (24U + face_0));

#line 686
                }

#line 675
                face_0 = face_0 + 1U;

#line 675
            }

#line 673
        }
        else
        {

#line 673
            entry_0 = index_0;

#line 673
        }

#line 657
    }
    else
    {

#line 693
        if((((&kernelContext_3)->cull_0->features_0) & 1U) != 0U)
        {

#line 693
            face_0 = index_0 | 1056964608U;

#line 693
        }
        else
        {

#line 693
            face_0 = index_0;

#line 693
        }

#line 693
        entry_0 = face_0;

#line 657
    }

#line 700
    uint slot_0 = atomic_fetch_add_explicit((&kernelContext_3)->visible_count_0+0U, 1U, memory_order_relaxed);
    if(slot_0 < ((&kernelContext_3)->cull_0->capacity_0))
    {
        *((&kernelContext_3)->visible_0+slot_0) = entry_0;

#line 701
    }



    return;
}


#line 470
struct ScreenBounds_0
{
    bool valid_0;
    float min_x_1;
    float min_y_1;
    float max_x_1;
    float max_y_1;
    float nearest_0;
};


#line 489
ScreenBounds_0 project_box_0(matrix<float,int(4),int(4)>  view_proj_1, float3 center_3, float3 extent_3, float width_0, float height_0)
{
    thread ScreenBounds_0 bounds_0;
    (&bounds_0)->valid_0 = true;
    (&bounds_0)->min_x_1 = 1.6777216e+07f;
    (&bounds_0)->min_y_1 = 1.6777216e+07f;
    (&bounds_0)->max_x_1 = -1.6777216e+07f;
    (&bounds_0)->max_y_1 = -1.6777216e+07f;
    (&bounds_0)->nearest_0 = 0.0f;

#line 497
    uint corner_0 = 0U;
    for(;;)
    {

#line 498
        if(corner_0 < 8U)
        {
        }
        else
        {

#line 498
            break;
        }

#line 498
        float _S15;


        if((corner_0 & 1U) != 0U)
        {

#line 501
            _S15 = 1.0f;

#line 501
        }
        else
        {

#line 501
            _S15 = -1.0f;

#line 501
        }

#line 501
        float _S16;
        if((corner_0 & 2U) != 0U)
        {

#line 502
            _S16 = 1.0f;

#line 502
        }
        else
        {

#line 502
            _S16 = -1.0f;

#line 502
        }

#line 502
        float _S17;
        if((corner_0 & 4U) != 0U)
        {

#line 503
            _S17 = 1.0f;

#line 503
        }
        else
        {

#line 503
            _S17 = -1.0f;

#line 503
        }
        float4 clip_0 = (((float4(center_3 + float3(_S15, _S16, _S17) * extent_3, 1.0f)) * (view_proj_1)));


        float _S18 = clip_0.w;

#line 507
        if(!(_S18 > 0.0f))
        {
            (&bounds_0)->valid_0 = false;
            return bounds_0;
        }
        float depth_0 = clip_0.z / _S18;
        float x_1 = (clip_0.x / _S18 * 0.5f + 0.5f) * width_0;
        float y_0 = (0.5f - clip_0.y / _S18 * 0.5f) * height_0;

#line 514
        bool _S19;
        if(!(depth_0 <= 1.0f))
        {

#line 515
            _S19 = true;

#line 515
        }
        else
        {

#line 515
            _S19 = !((abs(x_1)) < 1.6777216e+07f);

#line 515
        }

#line 515
        bool _S20;

#line 515
        if(_S19)
        {

#line 515
            _S20 = true;

#line 515
        }
        else
        {

#line 515
            _S20 = !((abs(y_0)) < 1.6777216e+07f);

#line 515
        }

#line 515
        if(_S20)
        {
            (&bounds_0)->valid_0 = false;
            return bounds_0;
        }
        (&bounds_0)->min_x_1 = min((&bounds_0)->min_x_1, x_1);
        (&bounds_0)->min_y_1 = min((&bounds_0)->min_y_1, y_0);
        (&bounds_0)->max_x_1 = max((&bounds_0)->max_x_1, x_1);
        (&bounds_0)->max_y_1 = max((&bounds_0)->max_y_1, y_0);
        (&bounds_0)->nearest_0 = max((&bounds_0)->nearest_0, depth_0);

#line 498
        corner_0 = corner_0 + 1U;

#line 498
    }

#line 526
    return bounds_0;
}




float pyramid_load_0(uint level_0, int2 texel_0, KernelContext_0 thread* kernelContext_4)
{
    int3 at_0 = int3(texel_0, int(0));
    switch(level_0)
    {
    case 1U:
        {

#line 538
            return ((kernelContext_4->pyramid_1_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 2U:
        {

#line 540
            return ((kernelContext_4->pyramid_2_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 3U:
        {

#line 542
            return ((kernelContext_4->pyramid_3_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 4U:
        {

#line 544
            return ((kernelContext_4->pyramid_4_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 5U:
        {

#line 546
            return ((kernelContext_4->pyramid_5_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 6U:
        {

#line 548
            return ((kernelContext_4->pyramid_6_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    case 7U:
        {

#line 550
            return ((kernelContext_4->pyramid_7_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    default:
        {

#line 552
            return ((kernelContext_4->pyramid_8_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));
        }
    }

#line 552
}


#line 572
bool occluded_0(matrix<float,int(4),int(4)>  view_proj_2, float3 center_4, float3 extent_4, KernelContext_0 thread* kernelContext_5)
{
    if((kernelContext_5->cull_0->pyramid_levels_0) == 0U)
    {
        return false;
    }
    ScreenBounds_0 bounds_1 = project_box_0(view_proj_2, center_4, extent_4, float(kernelContext_5->cull_0->target_width_0), float(kernelContext_5->cull_0->target_height_0));

    if(!bounds_1.valid_0)
    {
        return false;
    }
    int width_1 = int(kernelContext_5->cull_0->target_width_0);
    int height_1 = int(kernelContext_5->cull_0->target_height_0);
    int x0_0 = int(floor(bounds_1.min_x_1)) - int(1);
    int y0_0 = int(floor(bounds_1.min_y_1)) - int(1);
    int x1_0 = int(floor(bounds_1.max_x_1)) + int(1);
    int y1_0 = int(floor(bounds_1.max_y_1)) + int(1);

#line 589
    bool _S21;


    if(x1_0 < int(0))
    {

#line 592
        _S21 = true;

#line 592
    }
    else
    {

#line 592
        _S21 = y1_0 < int(0);

#line 592
    }

#line 592
    if(_S21)
    {

#line 592
        _S21 = true;

#line 592
    }
    else
    {

#line 592
        _S21 = x0_0 >= width_1;

#line 592
    }

#line 592
    if(_S21)
    {

#line 592
        _S21 = true;

#line 592
    }
    else
    {

#line 592
        _S21 = y0_0 >= height_1;

#line 592
    }

#line 592
    if(_S21)
    {
        return false;
    }
    uint left_0 = uint(max(x0_0, int(0)));
    uint top_0 = uint(max(y0_0, int(0)));
    uint right_0 = uint(min(x1_0, width_1 - int(1)));
    uint bottom_0 = uint(min(y1_0, height_1 - int(1)));

#line 599
    uint level_1 = 1U;


    for(;;)
    {

#line 603
        if(level_1 < (kernelContext_5->cull_0->pyramid_levels_0))
        {

#line 603
            if(((right_0 >> level_1) - (left_0 >> level_1)) > 1U)
            {

#line 603
                _S21 = true;

#line 603
            }
            else
            {

#line 603
                _S21 = ((bottom_0 >> level_1) - (top_0 >> level_1)) > 1U;

#line 603
            }

#line 603
        }
        else
        {

#line 603
            _S21 = false;

#line 603
        }

#line 602
        if(_S21)
        {
        }
        else
        {

#line 602
            break;
        }

#line 602
        level_1 = level_1 + 1U;

#line 602
    }

#line 609
    uint _S22 = max((kernelContext_5->cull_0->target_width_0) >> level_1, 1U) - 1U;

#line 609
    uint _S23 = min(left_0 >> level_1, _S22);
    uint _S24 = max((kernelContext_5->cull_0->target_height_0) >> level_1, 1U) - 1U;

#line 610
    uint _S25 = min(top_0 >> level_1, _S24);
    uint _S26 = min(right_0 >> level_1, _S22);
    uint _S27 = min(bottom_0 >> level_1, _S24);

#line 612
    float farthest_0 = 1.0f;

#line 612
    uint y_1 = _S25;



    for(;;)
    {

#line 616
        if(y_1 <= _S27)
        {
        }
        else
        {

#line 616
            break;
        }

#line 616
        uint x_2 = _S23;

        for(;;)
        {

#line 618
            if(x_2 <= _S26)
            {
            }
            else
            {

#line 618
                break;
            }

#line 618
            float _S28 = pyramid_load_0(level_1, int2(int(x_2), int(y_1)), kernelContext_5);

            float _S29 = min(farthest_0, _S28);

#line 618
            uint x_3 = x_2 + 1U;

#line 618
            farthest_0 = _S29;

#line 618
            x_2 = x_3;

#line 618
        }

#line 616
        y_1 = y_1 + 1U;

#line 616
    }

#line 623
    return (bounds_1.nearest_0 * 1.000244140625f) < farthest_0;
}


#line 720
[[kernel]] void occlusionMain(uint3 thread_1 [[thread_position_in_grid]], CullParams_natural_0 constant* cull_2 [[buffer(0)]], GpuInstance_natural_0 device* instances_2 [[buffer(1)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], atomic<uint> device* visible_count_2 [[buffer(4)]], uint device* visible_2 [[buffer(3)]], depth2d<float, access::sample> pyramid_1_2 [[texture(0)]], depth2d<float, access::sample> pyramid_2_2 [[texture(1)]], depth2d<float, access::sample> pyramid_3_2 [[texture(2)]], depth2d<float, access::sample> pyramid_4_2 [[texture(3)]], depth2d<float, access::sample> pyramid_5_2 [[texture(4)]], depth2d<float, access::sample> pyramid_6_2 [[texture(5)]], depth2d<float, access::sample> pyramid_7_2 [[texture(6)]], depth2d<float, access::sample> pyramid_8_2 [[texture(7)]])
{

#line 720
    thread KernelContext_0 kernelContext_6;

#line 720
    (&kernelContext_6)->cull_0 = cull_2;

#line 720
    (&kernelContext_6)->instances_0 = instances_2;

#line 720
    (&kernelContext_6)->meshes_0 = meshes_2;

#line 720
    (&kernelContext_6)->visible_count_0 = visible_count_2;

#line 720
    (&kernelContext_6)->visible_0 = visible_2;

#line 720
    (&kernelContext_6)->pyramid_1_0 = pyramid_1_2;

#line 720
    (&kernelContext_6)->pyramid_2_0 = pyramid_2_2;

#line 720
    (&kernelContext_6)->pyramid_3_0 = pyramid_3_2;

#line 720
    (&kernelContext_6)->pyramid_4_0 = pyramid_4_2;

#line 720
    (&kernelContext_6)->pyramid_5_0 = pyramid_5_2;

#line 720
    (&kernelContext_6)->pyramid_6_0 = pyramid_6_2;

#line 720
    (&kernelContext_6)->pyramid_7_0 = pyramid_7_2;

#line 720
    (&kernelContext_6)->pyramid_8_0 = pyramid_8_2;

    uint index_1 = thread_1.x;
    if(index_1 >= (cull_2->instance_count_0))
    {
        return;
    }

#line 725
    GpuInstance_natural_0 device* _S30 = (&kernelContext_6)->instances_0+index_1;

#line 725
    bool _S31 = admits_0(index_1, &kernelContext_6);



    if(!_S31)
    {
        return;
    }
    GpuMesh_0 mesh_2 = (&kernelContext_6)->meshes_0[_S30->mesh_0];

#line 739
    if((mesh_2.index_count_0) == 0U)
    {
        return;
    }

#line 741
    uint entry_1;



    if(((_S30->flags_0) & 2U) == 0U)
    {

#line 745
        thread GpuMesh_0 _S32 = mesh_2;

        thread float3 center_5;
        thread float3 extent_5;

#line 748
        world_box_0(index_1, &_S32, &center_5, &extent_5, &kernelContext_6);

#line 748
        bool _S33 = in_frustum_0(center_5, extent_5, &kernelContext_6);

        if(!_S33)
        {
            return;
        }

#line 752
        bool _S34;

        if((((&kernelContext_6)->cull_0->features_0) & 4U) != 0U)
        {
            ScreenBounds_0 bounds_2 = project_box_0(matrix<float,int(4),int(4)> ((&kernelContext_6)->cull_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->cull_0->view_proj_0.data_0[int(3)][int(3)]), center_5, extent_5, float((&kernelContext_6)->cull_0->target_width_0), float((&kernelContext_6)->cull_0->target_height_0));


            if(bounds_2.valid_0)
            {

#line 759
                _S34 = (max(bounds_2.max_x_1 - bounds_2.min_x_1, bounds_2.max_y_1 - bounds_2.min_y_1)) < ((&kernelContext_6)->cull_0->small_feature_pixels_0);

#line 759
            }
            else
            {

#line 759
                _S34 = false;

#line 759
            }

#line 758
            if(_S34)
            {


                uint _S35 = atomic_fetch_add_explicit((&kernelContext_6)->visible_count_0+7U, 1U, memory_order_relaxed);
                return;
            }

#line 754
        }

#line 767
        if((((&kernelContext_6)->cull_0->features_0) & 2U) != 0U)
        {

#line 767
            bool _S36 = occluded_0(matrix<float,int(4),int(4)> ((&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->cull_0->previous_view_proj_0.data_0[int(3)][int(3)]), center_5, extent_5, &kernelContext_6);

#line 767
            _S34 = _S36;

#line 767
        }
        else
        {

#line 767
            _S34 = false;

#line 767
        }

#line 766
        if(_S34)
        {

            uint entry_2 = index_1 | 2147483648U;
            uint _S37 = atomic_fetch_add_explicit((&kernelContext_6)->visible_count_0+5U, 1U, memory_order_relaxed);

#line 770
            entry_1 = entry_2;

#line 766
        }
        else
        {

#line 766
            entry_1 = index_1;

#line 766
        }

#line 745
    }
    else
    {

#line 745
        entry_1 = index_1;

#line 745
    }

#line 774
    uint slot_1 = atomic_fetch_add_explicit((&kernelContext_6)->visible_count_0+0U, 1U, memory_order_relaxed);
    if(slot_1 < ((&kernelContext_6)->cull_0->capacity_0))
    {
        *((&kernelContext_6)->visible_0+slot_1) = entry_1;

#line 775
    }



    return;
}


#line 793
[[kernel]] void lateMain(uint3 thread_2 [[thread_position_in_grid]], CullParams_natural_0 constant* cull_3 [[buffer(0)]], GpuInstance_natural_0 device* instances_3 [[buffer(1)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], atomic<uint> device* visible_count_3 [[buffer(4)]], uint device* visible_3 [[buffer(3)]], depth2d<float, access::sample> pyramid_1_3 [[texture(0)]], depth2d<float, access::sample> pyramid_2_3 [[texture(1)]], depth2d<float, access::sample> pyramid_3_3 [[texture(2)]], depth2d<float, access::sample> pyramid_4_3 [[texture(3)]], depth2d<float, access::sample> pyramid_5_3 [[texture(4)]], depth2d<float, access::sample> pyramid_6_3 [[texture(5)]], depth2d<float, access::sample> pyramid_7_3 [[texture(6)]], depth2d<float, access::sample> pyramid_8_3 [[texture(7)]])
{

#line 793
    thread KernelContext_0 kernelContext_7;

#line 793
    (&kernelContext_7)->cull_0 = cull_3;

#line 793
    (&kernelContext_7)->instances_0 = instances_3;

#line 793
    (&kernelContext_7)->meshes_0 = meshes_3;

#line 793
    (&kernelContext_7)->visible_count_0 = visible_count_3;

#line 793
    (&kernelContext_7)->visible_0 = visible_3;

#line 793
    (&kernelContext_7)->pyramid_1_0 = pyramid_1_3;

#line 793
    (&kernelContext_7)->pyramid_2_0 = pyramid_2_3;

#line 793
    (&kernelContext_7)->pyramid_3_0 = pyramid_3_3;

#line 793
    (&kernelContext_7)->pyramid_4_0 = pyramid_4_3;

#line 793
    (&kernelContext_7)->pyramid_5_0 = pyramid_5_3;

#line 793
    (&kernelContext_7)->pyramid_6_0 = pyramid_6_3;

#line 793
    (&kernelContext_7)->pyramid_7_0 = pyramid_7_3;

#line 793
    (&kernelContext_7)->pyramid_8_0 = pyramid_8_3;

    uint index_2 = thread_2.x;
    uint _S38 = atomic_load_explicit(visible_count_3+0U, memory_order_relaxed);

#line 796
    if(index_2 >= (min(_S38, (&kernelContext_7)->cull_0->capacity_0)))
    {
        return;
    }
    uint device* _S39 = (&kernelContext_7)->visible_0+index_2;

#line 800
    uint entry_3 = *_S39;
    if(((*_S39) & 2147483648U) == 0U)
    {
        return;
    }
    uint _S40 = entry_3 & 16777215U;

#line 805
    thread GpuMesh_0 _S41 = (&kernelContext_7)->meshes_0[((&kernelContext_7)->instances_0+_S40)->mesh_0];

    thread float3 center_6;
    thread float3 extent_6;

#line 808
    world_box_0(_S40, &_S41, &center_6, &extent_6, &kernelContext_7);

#line 808
    bool _S42 = occluded_0(matrix<float,int(4),int(4)> ((&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_7)->cull_0->view_proj_0.data_0[int(3)][int(3)]), center_6, extent_6, &kernelContext_7);

    if(_S42)
    {
        uint _S43 = atomic_fetch_add_explicit((&kernelContext_7)->visible_count_0+6U, 1U, memory_order_relaxed);
        return;
    }
    *((&kernelContext_7)->visible_0+index_2) = entry_3 | 1073741824U;
    return;
}

