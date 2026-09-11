#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
int main(int argc, const char **argv) {
  @autoreleasepool {
    if (argc != 3)
      return 2;
    id<MTLDevice> d = MTLCreateSystemDefaultDevice();
    if (!d)
      return 3;
    NSError *e = nil;
    MTLCompileOptions *o = [MTLCompileOptions new];
    o.languageVersion = MTLLanguageVersion3_1;
    NSString *a = [NSString stringWithContentsOfFile:@(argv[1])
                                            encoding:NSUTF8StringEncoding
                                               error:&e];
    NSString *b = [NSString stringWithContentsOfFile:@(argv[2])
                                            encoding:NSUTF8StringEncoding
                                               error:&e];
    id<MTLLibrary> ml = [d newLibraryWithSource:a options:o error:&e];
    if (!ml) {
      NSLog(@"%@", e);
      return 4;
    }
    id<MTLLibrary> fl = [d newLibraryWithSource:b options:o error:&e];
    if (!fl) {
      NSLog(@"%@", e);
      return 5;
    }
    MTLMeshRenderPipelineDescriptor *p = [MTLMeshRenderPipelineDescriptor new];
    p.meshFunction = [ml newFunctionWithName:@"meshMain"];
    p.fragmentFunction = [fl newFunctionWithName:@"fragmentMain"];
    p.colorAttachments[0].pixelFormat = MTLPixelFormatRGBA16Float;
    p.colorAttachments[1].pixelFormat = MTLPixelFormatRG16Float;
    id<MTLRenderPipelineState> ps =
        [d newRenderPipelineStateWithMeshDescriptor:p
                                            options:0
                                         reflection:nil
                                              error:&e];
    if (!ps) {
      NSLog(@"%@", e);
      return 6;
    }
    MTLRenderPipelineDescriptor *r = [MTLRenderPipelineDescriptor new];
    r.vertexFunction = [fl newFunctionWithName:@"vertexMain"];
    r.fragmentFunction = p.fragmentFunction;
    r.colorAttachments[0].pixelFormat = p.colorAttachments[0].pixelFormat;
    r.colorAttachments[1].pixelFormat = p.colorAttachments[1].pixelFormat;
    if (![d newRenderPipelineStateWithDescriptor:r error:&e]) {
      NSLog(@"%@", e);
      return 7;
    }
    NSLog(@"PASS native mesh and vertex stage linking on %@", d.name);
    return 0;
  }
}
