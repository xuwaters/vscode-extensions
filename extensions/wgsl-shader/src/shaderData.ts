// The word lists behind completion, for both shader languages. Kept free of the
// `vscode` module so they can be unit tested, and kept in step with the two
// TextMate grammars in syntaxes/.

// ── WGSL ───────────────────────────────────────────────────────────

export const WGSL_BUILTIN_FUNCTIONS = [
  // math
  'abs', 'acos', 'acosh', 'asin', 'asinh', 'atan', 'atanh', 'atan2',
  'ceil', 'clamp', 'cos', 'cosh', 'countLeadingZeros', 'countOneBits',
  'countTrailingZeros', 'cross', 'degrees', 'determinant', 'distance',
  'dot', 'exp', 'exp2', 'extractBits', 'faceForward', 'firstLeadingBit',
  'firstTrailingBit', 'floor', 'fma', 'fract', 'frexp', 'insertBits',
  'inverseSqrt', 'ldexp', 'length', 'log', 'log2', 'max', 'min', 'mix',
  'modf', 'normalize', 'pow', 'quantizeToF16', 'radians', 'reflect',
  'refract', 'reverseBits', 'round', 'saturate', 'sign', 'sin', 'sinh',
  'smoothstep', 'sqrt', 'step', 'tan', 'tanh', 'transpose', 'trunc',
  // texture
  'textureDimensions', 'textureGather', 'textureGatherCompare',
  'textureLoad', 'textureNumLayers', 'textureNumLevels',
  'textureNumSamples', 'textureSample', 'textureSampleBias',
  'textureSampleCompare', 'textureSampleCompareLevel',
  'textureSampleGrad', 'textureSampleLevel', 'textureStore',
  // atomic
  'atomicLoad', 'atomicStore', 'atomicAdd', 'atomicSub', 'atomicMax',
  'atomicMin', 'atomicAnd', 'atomicOr', 'atomicXor', 'atomicExchange',
  'atomicCompareExchangeWeak',
  // data packing
  'pack2x16float', 'pack2x16snorm', 'pack2x16unorm', 'pack4x8snorm',
  'pack4x8unorm', 'unpack2x16float', 'unpack2x16snorm',
  'unpack2x16unorm', 'unpack4x8snorm', 'unpack4x8unorm',
  // synchronization
  'storageBarrier', 'workgroupBarrier', 'workgroupUniformLoad',
  // construction / conversion
  'bitcast', 'select', 'arrayLength',
];

export const WGSL_BUILTIN_TYPES = [
  'bool', 'f16', 'f32', 'i32', 'u32',
  'vec2', 'vec3', 'vec4',
  'vec2i', 'vec3i', 'vec4i', 'vec2u', 'vec3u', 'vec4u',
  'vec2f', 'vec3f', 'vec4f', 'vec2h', 'vec3h', 'vec4h',
  'mat2x2', 'mat2x3', 'mat2x4', 'mat3x2', 'mat3x3', 'mat3x4',
  'mat4x2', 'mat4x3', 'mat4x4',
  'mat2x2f', 'mat2x3f', 'mat2x4f', 'mat3x2f', 'mat3x3f', 'mat3x4f',
  'mat4x2f', 'mat4x3f', 'mat4x4f',
  'mat2x2h', 'mat2x3h', 'mat2x4h', 'mat3x2h', 'mat3x3h', 'mat3x4h',
  'mat4x2h', 'mat4x3h', 'mat4x4h',
  'array', 'atomic', 'ptr',
  'sampler', 'sampler_comparison',
  'texture_1d', 'texture_2d', 'texture_2d_array', 'texture_3d',
  'texture_cube', 'texture_cube_array', 'texture_multisampled_2d',
  'texture_storage_1d', 'texture_storage_2d', 'texture_storage_2d_array',
  'texture_storage_3d', 'texture_depth_2d', 'texture_depth_2d_array',
  'texture_depth_cube', 'texture_depth_multisampled_2d', 'texture_external',
];

export const WGSL_KEYWORDS = [
  'fn', 'let', 'var', 'const', 'override', 'struct', 'alias',
  'if', 'else', 'for', 'while', 'loop', 'break', 'continue', 'continuing',
  'return', 'discard', 'switch', 'case', 'default', 'fallthrough',
  'enable', 'requires', 'diagnostic', 'const_assert',
  'true', 'false',
];

export const WGSL_ATTRIBUTES = [
  'align', 'binding', 'builtin', 'compute', 'const', 'diagnostic',
  'fragment', 'group', 'id', 'interpolate', 'invariant', 'location',
  'must_use', 'size', 'vertex', 'workgroup_size',
];

// ── GLSL ───────────────────────────────────────────────────────────

export const GLSL_BUILTIN_FUNCTIONS = [
  // angle and trigonometry
  'radians', 'degrees', 'sin', 'cos', 'tan', 'asin', 'acos', 'atan',
  'sinh', 'cosh', 'tanh', 'asinh', 'acosh', 'atanh',
  // exponential
  'pow', 'exp', 'log', 'exp2', 'log2', 'sqrt', 'inversesqrt',
  // common
  'abs', 'sign', 'floor', 'trunc', 'round', 'roundEven', 'ceil', 'fract',
  'mod', 'modf', 'min', 'max', 'clamp', 'mix', 'step', 'smoothstep',
  'isnan', 'isinf', 'floatBitsToInt', 'floatBitsToUint', 'intBitsToFloat',
  'uintBitsToFloat', 'fma', 'frexp', 'ldexp',
  // packing
  'packUnorm2x16', 'packSnorm2x16', 'packUnorm4x8', 'packSnorm4x8',
  'unpackUnorm2x16', 'unpackSnorm2x16', 'unpackUnorm4x8', 'unpackSnorm4x8',
  'packHalf2x16', 'unpackHalf2x16', 'packDouble2x32', 'unpackDouble2x32',
  // geometric
  'length', 'distance', 'dot', 'cross', 'normalize', 'faceforward',
  'reflect', 'refract',
  // matrix
  'matrixCompMult', 'outerProduct', 'transpose', 'determinant', 'inverse',
  // vector relational
  'lessThan', 'lessThanEqual', 'greaterThan', 'greaterThanEqual', 'equal',
  'notEqual', 'any', 'all', 'not',
  // integer
  'uaddCarry', 'usubBorrow', 'umulExtended', 'imulExtended',
  'bitfieldExtract', 'bitfieldInsert', 'bitfieldReverse', 'bitCount',
  'findLSB', 'findMSB',
  // texture
  'textureSize', 'textureQueryLod', 'textureQueryLevels', 'textureSamples',
  'texture', 'textureProj', 'textureLod', 'textureOffset', 'texelFetch',
  'texelFetchOffset', 'textureProjOffset', 'textureLodOffset',
  'textureProjLod', 'textureProjLodOffset', 'textureGrad',
  'textureGradOffset', 'textureProjGrad', 'textureProjGradOffset',
  'textureGather', 'textureGatherOffset', 'textureGatherOffsets',
  // atomic counters and atomics
  'atomicCounterIncrement', 'atomicCounterDecrement', 'atomicCounter',
  'atomicAdd', 'atomicMin', 'atomicMax', 'atomicAnd', 'atomicOr',
  'atomicXor', 'atomicExchange', 'atomicCompSwap',
  // images
  'imageSize', 'imageSamples', 'imageLoad', 'imageStore', 'imageAtomicAdd',
  'imageAtomicMin', 'imageAtomicMax', 'imageAtomicAnd', 'imageAtomicOr',
  'imageAtomicXor', 'imageAtomicExchange', 'imageAtomicCompSwap',
  'subpassLoad',
  // fragment processing
  'dFdx', 'dFdy', 'dFdxFine', 'dFdyFine', 'dFdxCoarse', 'dFdyCoarse',
  'fwidth', 'fwidthFine', 'fwidthCoarse', 'interpolateAtCentroid',
  'interpolateAtSample', 'interpolateAtOffset',
  // synchronization
  'barrier', 'memoryBarrier', 'memoryBarrierAtomicCounter',
  'memoryBarrierBuffer', 'memoryBarrierShared', 'memoryBarrierImage',
  'groupMemoryBarrier',
  // geometry shader
  'EmitVertex', 'EndPrimitive', 'EmitStreamVertex', 'EndStreamPrimitive',
];

const SAMPLER_SHAPES = [
  '1D', '2D', '3D', 'Cube', '2DRect', '1DArray', '2DArray', 'CubeArray',
  'Buffer', '2DMS', '2DMSArray',
];

const SHADOW_SHAPES = [
  '1D', '2D', 'Cube', '2DRect', '1DArray', '2DArray', 'CubeArray',
];

/** `sampler2D`, `isampler2D`, `usampler2D`, `sampler2DShadow`, … */
function opaqueTypes(base: string, shapes: string[], shadow = false): string[] {
  const names: string[] = [];
  for (const prefix of ['', 'i', 'u']) {
    for (const shape of shapes) {
      names.push(`${prefix}${base}${shape}`);
    }
  }
  if (shadow) {
    for (const shape of SHADOW_SHAPES) {
      names.push(`${base}${shape}Shadow`);
    }
  }
  return names;
}

export const GLSL_BUILTIN_TYPES = [
  'void', 'bool', 'int', 'uint', 'float', 'double', 'atomic_uint',
  'vec2', 'vec3', 'vec4',
  'bvec2', 'bvec3', 'bvec4',
  'ivec2', 'ivec3', 'ivec4',
  'uvec2', 'uvec3', 'uvec4',
  'dvec2', 'dvec3', 'dvec4',
  'mat2', 'mat3', 'mat4',
  'mat2x2', 'mat2x3', 'mat2x4', 'mat3x2', 'mat3x3', 'mat3x4',
  'mat4x2', 'mat4x3', 'mat4x4',
  'dmat2', 'dmat3', 'dmat4',
  'dmat2x2', 'dmat2x3', 'dmat2x4', 'dmat3x2', 'dmat3x3', 'dmat3x4',
  'dmat4x2', 'dmat4x3', 'dmat4x4',
  ...opaqueTypes('sampler', SAMPLER_SHAPES, true),
  ...opaqueTypes('image', SAMPLER_SHAPES),
  ...opaqueTypes('texture', SAMPLER_SHAPES),
  'sampler', 'samplerShadow', 'subpassInput', 'subpassInputMS',
];

export const GLSL_KEYWORDS = [
  'if', 'else', 'for', 'while', 'do', 'switch', 'case', 'default',
  'break', 'continue', 'return', 'discard', 'struct',
  'const', 'uniform', 'buffer', 'shared', 'in', 'out', 'inout',
  'attribute', 'varying', 'layout', 'subroutine',
  'centroid', 'flat', 'smooth', 'noperspective', 'invariant', 'precise',
  'patch', 'sample', 'coherent', 'volatile', 'restrict', 'readonly',
  'writeonly', 'precision', 'highp', 'mediump', 'lowp',
  'true', 'false',
];

/** Names valid inside `layout(...)`, offered when completing there. */
export const GLSL_LAYOUT_QUALIFIERS = [
  'location', 'binding', 'set', 'offset', 'align', 'component', 'index',
  'input_attachment_index', 'push_constant', 'constant_id',
  'std140', 'std430', 'packed', 'shared', 'row_major', 'column_major',
  'local_size_x', 'local_size_y', 'local_size_z',
  'points', 'lines', 'lines_adjacency', 'triangles', 'triangles_adjacency',
  'line_strip', 'triangle_strip', 'max_vertices', 'invocations', 'stream',
  'vertices', 'quads', 'isolines', 'equal_spacing',
  'fractional_even_spacing', 'fractional_odd_spacing', 'cw', 'ccw',
  'point_mode', 'origin_upper_left', 'pixel_center_integer',
  'early_fragment_tests', 'depth_any', 'depth_greater', 'depth_less',
  'depth_unchanged',
];

/** `gl_`-prefixed built-in variables, tagged with the stages they belong to. */
export const GLSL_BUILTIN_VARIABLES: Array<{ name: string; stages: string }> = [
  { name: 'gl_Position', stages: 'vertex' },
  { name: 'gl_PointSize', stages: 'vertex' },
  { name: 'gl_VertexID', stages: 'vertex' },
  { name: 'gl_InstanceID', stages: 'vertex' },
  { name: 'gl_VertexIndex', stages: 'vertex (Vulkan)' },
  { name: 'gl_InstanceIndex', stages: 'vertex (Vulkan)' },
  { name: 'gl_DrawID', stages: 'vertex' },
  { name: 'gl_BaseVertex', stages: 'vertex' },
  { name: 'gl_BaseInstance', stages: 'vertex' },
  { name: 'gl_ClipDistance', stages: 'vertex, fragment' },
  { name: 'gl_CullDistance', stages: 'vertex, fragment' },
  { name: 'gl_FragCoord', stages: 'fragment' },
  { name: 'gl_FrontFacing', stages: 'fragment' },
  { name: 'gl_PointCoord', stages: 'fragment' },
  { name: 'gl_FragDepth', stages: 'fragment' },
  { name: 'gl_SampleID', stages: 'fragment' },
  { name: 'gl_SamplePosition', stages: 'fragment' },
  { name: 'gl_SampleMask', stages: 'fragment' },
  { name: 'gl_SampleMaskIn', stages: 'fragment' },
  { name: 'gl_PrimitiveID', stages: 'fragment, geometry' },
  { name: 'gl_Layer', stages: 'fragment, geometry' },
  { name: 'gl_ViewportIndex', stages: 'fragment, geometry' },
  { name: 'gl_FragColor', stages: 'fragment (legacy)' },
  { name: 'gl_FragData', stages: 'fragment (legacy)' },
  { name: 'gl_NumWorkGroups', stages: 'compute' },
  { name: 'gl_WorkGroupSize', stages: 'compute' },
  { name: 'gl_WorkGroupID', stages: 'compute' },
  { name: 'gl_LocalInvocationID', stages: 'compute' },
  { name: 'gl_GlobalInvocationID', stages: 'compute' },
  { name: 'gl_LocalInvocationIndex', stages: 'compute' },
  { name: 'gl_in', stages: 'geometry, tessellation' },
  { name: 'gl_out', stages: 'tessellation control' },
  { name: 'gl_InvocationID', stages: 'geometry, tessellation control' },
  { name: 'gl_PatchVerticesIn', stages: 'tessellation' },
  { name: 'gl_TessCoord', stages: 'tessellation evaluation' },
  { name: 'gl_TessLevelOuter', stages: 'tessellation' },
  { name: 'gl_TessLevelInner', stages: 'tessellation' },
];

export const GLSL_DIRECTIVES = [
  'version', 'define', 'undef', 'if', 'ifdef', 'ifndef', 'else', 'elif',
  'endif', 'error', 'pragma', 'extension', 'line', 'include',
];
