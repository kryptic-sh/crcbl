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


#line 73 "shaders/atlas_view.slang"
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


#line 172 "shaders/atlas_view.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], AtlasViewParams_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(0)]])
{

#line 172
    thread KernelContext_0 kernelContext_0;

#line 172
    (&kernelContext_0)->params_0 = params_1;

#line 172
    (&kernelContext_0)->shadow_atlas_0 = shadow_atlas_1;

#line 178
    float2 inside_0 = position_0.xy - params_1->view_0.xy;

#line 178
    bool _S2;


    if(any(inside_0 < float2(0.0f, 0.0f)))
    {

#line 181
        _S2 = true;

#line 181
    }
    else
    {

#line 181
        _S2 = any(inside_0 >= (params_1->view_0.zw));

#line 181
    }

#line 181
    if(_S2)
    {

#line 181
        pixelOutput_0 _S3 = { float4(0.0f, 0.0f, 0.0f, 1.0f) };

        return _S3;
    }

#line 183
    uint slot_0 = 0U;

#line 190
    for(;;)
    {

#line 190
        if(slot_0 < 16U)
        {
        }
        else
        {

#line 190
            break;
        }
        float4 rect_1 = (&kernelContext_0)->params_0->rect_0[slot_0];



        if(((&kernelContext_0)->params_0->rect_0[slot_0].x) <= 0.0f)
        {

#line 196
            _S2 = true;

#line 196
        }
        else
        {

#line 196
            _S2 = (rect_1.y) <= 0.0f;

#line 196
        }

#line 196
        if(_S2)
        {
            slot_0 = slot_0 + 1U;

#line 190
            continue;
        }

#line 200
        float2 tile_min_0 = rect_1.zw * params_1->view_0.zw;
        float2 tile_max_0 = tile_min_0 + rect_1.xy * params_1->view_0.zw;

#line 201
        bool _S4;
        if(any(inside_0 < tile_min_0))
        {

#line 202
            _S4 = true;

#line 202
        }
        else
        {

#line 202
            _S4 = any(inside_0 >= tile_max_0);

#line 202
        }

#line 202
        if(_S4)
        {
            slot_0 = slot_0 + 1U;

#line 190
            continue;
        }

#line 206
        float2 edge_0 = min(inside_0 - tile_min_0, tile_max_0 - inside_0);
        if((min(edge_0.x, edge_0.y)) < 2.0f)
        {

#line 207
            pixelOutput_0 _S5 = { float4(float3(1.0f, 0.55000001192092896f, 0.15000000596046448f), 1.0f) };

            return _S5;
        }

#line 190
        slot_0 = slot_0 + 1U;

#line 190
    }

#line 213
    float2 extent_0 = (&kernelContext_0)->params_0->atlas_0.xy;

#line 219
    int3 _S6 = int3(int2(min(inside_0 / params_1->view_0.zw * extent_0, extent_0 - float2(1.0f, 1.0f))), int(0));

#line 219
    float depth_0 = (((&kernelContext_0)->shadow_atlas_0).read(vec<uint,2>(((_S6)).xy), uint(((_S6)).z)));

#line 219
    float grey_0;
    if(depth_0 > 0.0f)
    {

#line 220
        grey_0 = mix(0.30000001192092896f, 1.0f, depth_0);

#line 220
    }
    else
    {

#line 220
        grey_0 = 0.05999999865889549f;

#line 220
    }

#line 220
    pixelOutput_0 _S7 = { float4(grey_0, grey_0, grey_0, 1.0f) };
    return _S7;
}


#line 221
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 108
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 108
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], AtlasViewParams_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(0)]])
{

#line 108
    thread KernelContext_0 kernelContext_1;

#line 108
    (&kernelContext_1)->params_0 = params_2;

#line 108
    (&kernelContext_1)->shadow_atlas_0 = shadow_atlas_2;

#line 163
    thread FullscreenOutput_0 output_1;


    float2 _S8 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 166
    (&output_1)->uv_2 = _S8;
    (&output_1)->position_2 = float4(_S8 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 167
    thread vertexMain_Result_0 _S9;

#line 167
    (&_S9)->position_1 = output_1.position_2;

#line 167
    (&_S9)->uv_1 = output_1.uv_2;

#line 167
    return _S9;
}

