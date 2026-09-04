#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2580 "core.meta.slang"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 2580
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 61 "shaders/atlas_view.slang"
struct AtlasViewParams_0
{
    float4 view_0;
    float4 atlas_0;
    array<float4, int(16)> rect_0;
};


#line 1084 "core"
struct KernelContext_0
{
    AtlasViewParams_0 constant* params_0;
    depth2d<float, access::sample> shadow_atlas_0;
};


#line 160 "shaders/atlas_view.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], AtlasViewParams_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(0)]])
{

#line 160
    thread KernelContext_0 kernelContext_0;

#line 160
    (&kernelContext_0)->params_0 = params_1;

#line 160
    (&kernelContext_0)->shadow_atlas_0 = shadow_atlas_1;

#line 166
    float2 inside_0 = position_0.xy - params_1->view_0.xy;

#line 166
    bool _S2;


    if(any(inside_0 < float2(0.0f, 0.0f)))
    {

#line 169
        _S2 = true;

#line 169
    }
    else
    {

#line 169
        _S2 = any(inside_0 >= (params_1->view_0.zw));

#line 169
    }

#line 169
    if(_S2)
    {

#line 169
        pixelOutput_0 _S3 = { float4(0.0f, 0.0f, 0.0f, 1.0f) };

        return _S3;
    }

#line 171
    uint slot_0 = 0U;

#line 178
    for(;;)
    {

#line 178
        if(slot_0 < 16U)
        {
        }
        else
        {

#line 178
            break;
        }
        float4 rect_1 = (&kernelContext_0)->params_0->rect_0[slot_0];



        if(((&kernelContext_0)->params_0->rect_0[slot_0].x) <= 0.0f)
        {

#line 184
            _S2 = true;

#line 184
        }
        else
        {

#line 184
            _S2 = (rect_1.y) <= 0.0f;

#line 184
        }

#line 184
        if(_S2)
        {
            slot_0 = slot_0 + 1U;

#line 178
            continue;
        }

#line 188
        float2 tile_min_0 = rect_1.zw * params_1->view_0.zw;
        float2 tile_max_0 = tile_min_0 + rect_1.xy * params_1->view_0.zw;

#line 189
        bool _S4;
        if(any(inside_0 < tile_min_0))
        {

#line 190
            _S4 = true;

#line 190
        }
        else
        {

#line 190
            _S4 = any(inside_0 >= tile_max_0);

#line 190
        }

#line 190
        if(_S4)
        {
            slot_0 = slot_0 + 1U;

#line 178
            continue;
        }

#line 194
        float2 edge_0 = min(inside_0 - tile_min_0, tile_max_0 - inside_0);
        if((min(edge_0.x, edge_0.y)) < 2.0f)
        {

#line 195
            pixelOutput_0 _S5 = { float4(float3(1.0f, 0.55000001192092896f, 0.15000000596046448f), 1.0f) };

            return _S5;
        }

#line 178
        slot_0 = slot_0 + 1U;

#line 178
    }

#line 201
    float2 extent_0 = (&kernelContext_0)->params_0->atlas_0.xy;

#line 207
    int3 _S6 = int3(int2(min(inside_0 / params_1->view_0.zw * extent_0, extent_0 - float2(1.0f, 1.0f))), int(0));

#line 207
    float depth_0 = (((&kernelContext_0)->shadow_atlas_0).read(vec<uint,2>(((_S6)).xy), uint(((_S6)).z)));

#line 207
    float grey_0;
    if(depth_0 > 0.0f)
    {

#line 208
        grey_0 = mix(0.30000001192092896f, 1.0f, depth_0);

#line 208
    }
    else
    {

#line 208
        grey_0 = 0.05999999865889549f;

#line 208
    }

#line 208
    pixelOutput_0 _S7 = { float4(grey_0, grey_0, grey_0, 1.0f) };
    return _S7;
}


#line 209
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 96
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 96
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], AtlasViewParams_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(0)]])
{

#line 96
    thread KernelContext_0 kernelContext_1;

#line 96
    (&kernelContext_1)->params_0 = params_2;

#line 96
    (&kernelContext_1)->shadow_atlas_0 = shadow_atlas_2;

#line 151
    thread FullscreenOutput_0 output_1;


    float2 _S8 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 154
    (&output_1)->uv_2 = _S8;
    (&output_1)->position_2 = float4(_S8 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 155
    thread vertexMain_Result_0 _S9;

#line 155
    (&_S9)->position_1 = output_1.position_2;

#line 155
    (&_S9)->uv_1 = output_1.uv_2;

#line 155
    return _S9;
}

