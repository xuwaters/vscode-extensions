//! GLSL's predeclared names.
//!
//! Signatures use the spec's own generic notation: `genType` is `float` or a
//! `vecN`, `genIType`/`genUType`/`genBType` the integer, unsigned and boolean
//! families, `mat` any matrix.

use std::sync::OnceLock;

use super::{Builtin, builtins};

pub const FUNCTIONS: &[Builtin] = builtins![
    // ── Angle and trigonometry ────────────────────────────────────────────
    "radians", "radians(degrees: genType) -> genType", "Degrees converted to radians.";
    "degrees", "degrees(radians: genType) -> genType", "Radians converted to degrees.";
    "sin", "sin(angle: genType) -> genType", "Sine of an angle in radians.";
    "cos", "cos(angle: genType) -> genType", "Cosine of an angle in radians.";
    "tan", "tan(angle: genType) -> genType", "Tangent of an angle in radians.";
    "asin", "asin(x: genType) -> genType", "Arc sine, in radians.";
    "acos", "acos(x: genType) -> genType", "Arc cosine, in radians.";
    "atan", "atan(y: genType, x: genType) -> genType", "Arc tangent. The two-argument form picks the quadrant.";
    "sinh", "sinh(x: genType) -> genType", "Hyperbolic sine.";
    "cosh", "cosh(x: genType) -> genType", "Hyperbolic cosine.";
    "tanh", "tanh(x: genType) -> genType", "Hyperbolic tangent.";
    "asinh", "asinh(x: genType) -> genType", "Inverse hyperbolic sine.";
    "acosh", "acosh(x: genType) -> genType", "Inverse hyperbolic cosine.";
    "atanh", "atanh(x: genType) -> genType", "Inverse hyperbolic tangent.";

    // ── Exponential ───────────────────────────────────────────────────────
    "pow", "pow(x: genType, y: genType) -> genType", "x raised to the power y.";
    "exp", "exp(x: genType) -> genType", "e raised to the power x.";
    "log", "log(x: genType) -> genType", "Natural logarithm.";
    "exp2", "exp2(x: genType) -> genType", "2 raised to the power x.";
    "log2", "log2(x: genType) -> genType", "Base-2 logarithm.";
    "sqrt", "sqrt(x: genType) -> genType", "Square root.";
    "inversesqrt", "inversesqrt(x: genType) -> genType", "1 / sqrt(x).";

    // ── Common ────────────────────────────────────────────────────────────
    "abs", "abs(x: genType) -> genType", "Absolute value.";
    "sign", "sign(x: genType) -> genType", "-1, 0 or +1 by the sign of each component.";
    "floor", "floor(x: genType) -> genType", "Nearest integer at or below x.";
    "trunc", "trunc(x: genType) -> genType", "Fractional part discarded.";
    "round", "round(x: genType) -> genType", "Nearest integer; halfway cases are implementation-defined.";
    "roundEven", "roundEven(x: genType) -> genType", "Nearest integer, halfway cases to even.";
    "ceil", "ceil(x: genType) -> genType", "Nearest integer at or above x.";
    "fract", "fract(x: genType) -> genType", "x - floor(x).";
    "mod", "mod(x: genType, y: genType) -> genType", "x - y * floor(x / y). Not the C `%` operator.";
    "modf", "modf(x: genType, out i: genType) -> genType", "Splits x into fractional and whole parts.";
    "min", "min(x: genType, y: genType) -> genType", "Component-wise minimum.";
    "max", "max(x: genType, y: genType) -> genType", "Component-wise maximum.";
    "clamp", "clamp(x: genType, minVal: genType, maxVal: genType) -> genType", "x constrained to [minVal, maxVal].";
    "mix", "mix(x: genType, y: genType, a: genType) -> genType", "Linear blend: x * (1 - a) + y * a.";
    "step", "step(edge: genType, x: genType) -> genType", "0.0 below the edge, 1.0 at or above it.";
    "smoothstep", "smoothstep(edge0: genType, edge1: genType, x: genType) -> genType", "Hermite interpolation between two edges.";
    "isnan", "isnan(x: genType) -> genBType", "Whether each component is NaN.";
    "isinf", "isinf(x: genType) -> genBType", "Whether each component is infinite.";
    "floatBitsToInt", "floatBitsToInt(value: genType) -> genIType", "Reinterprets the bits as signed integers.";
    "floatBitsToUint", "floatBitsToUint(value: genType) -> genUType", "Reinterprets the bits as unsigned integers.";
    "intBitsToFloat", "intBitsToFloat(value: genIType) -> genType", "Reinterprets the bits as floats.";
    "uintBitsToFloat", "uintBitsToFloat(value: genUType) -> genType", "Reinterprets the bits as floats.";
    "fma", "fma(a: genType, b: genType, c: genType) -> genType", "Fused multiply-add: a * b + c, rounded once.";
    "frexp", "frexp(x: genType, out exp: genIType) -> genType", "Splits x into a fraction and an exponent.";
    "ldexp", "ldexp(x: genType, exp: genIType) -> genType", "x * 2^exp.";

    // ── Packing ───────────────────────────────────────────────────────────
    "packUnorm2x16", "packUnorm2x16(v: vec2) -> uint", "Two floats packed as unsigned normalised 16-bit ints.";
    "packSnorm2x16", "packSnorm2x16(v: vec2) -> uint", "Two floats packed as signed normalised 16-bit ints.";
    "packUnorm4x8", "packUnorm4x8(v: vec4) -> uint", "Four floats packed as unsigned normalised bytes.";
    "packSnorm4x8", "packSnorm4x8(v: vec4) -> uint", "Four floats packed as signed normalised bytes.";
    "unpackUnorm2x16", "unpackUnorm2x16(p: uint) -> vec2", "Unsigned normalised 16-bit ints unpacked to floats.";
    "unpackSnorm2x16", "unpackSnorm2x16(p: uint) -> vec2", "Signed normalised 16-bit ints unpacked to floats.";
    "unpackUnorm4x8", "unpackUnorm4x8(p: uint) -> vec4", "Unsigned normalised bytes unpacked to floats.";
    "unpackSnorm4x8", "unpackSnorm4x8(p: uint) -> vec4", "Signed normalised bytes unpacked to floats.";
    "packHalf2x16", "packHalf2x16(v: vec2) -> uint", "Two floats packed as two halves.";
    "unpackHalf2x16", "unpackHalf2x16(v: uint) -> vec2", "Two halves unpacked to floats.";
    "packDouble2x32", "packDouble2x32(v: uvec2) -> double", "Two uints reinterpreted as one double.";
    "unpackDouble2x32", "unpackDouble2x32(v: double) -> uvec2", "One double reinterpreted as two uints.";

    // ── Geometric ─────────────────────────────────────────────────────────
    "length", "length(x: genType) -> float", "Vector length.";
    "distance", "distance(p0: genType, p1: genType) -> float", "Euclidean distance between two points.";
    "dot", "dot(x: genType, y: genType) -> float", "Dot product.";
    "cross", "cross(x: vec3, y: vec3) -> vec3", "Cross product.";
    "normalize", "normalize(x: genType) -> genType", "Unit vector in the same direction.";
    "faceforward", "faceforward(N: genType, I: genType, Nref: genType) -> genType", "N flipped to face away from Nref.";
    "reflect", "reflect(I: genType, N: genType) -> genType", "Incident vector reflected about a normal.";
    "refract", "refract(I: genType, N: genType, eta: float) -> genType", "Refraction vector for a ratio of indices.";

    // ── Matrix ────────────────────────────────────────────────────────────
    "matrixCompMult", "matrixCompMult(x: mat, y: mat) -> mat", "Component-wise product. Not a matrix multiply.";
    "outerProduct", "outerProduct(c: vec, r: vec) -> mat", "Outer product of a column and a row vector.";
    "transpose", "transpose(m: mat) -> mat", "Matrix transpose.";
    "determinant", "determinant(m: mat) -> float", "Determinant of a square matrix.";
    "inverse", "inverse(m: mat) -> mat", "Matrix inverse. Undefined for a singular matrix.";

    // ── Vector relational ─────────────────────────────────────────────────
    "lessThan", "lessThan(x: vec, y: vec) -> bvec", "Component-wise `<`.";
    "lessThanEqual", "lessThanEqual(x: vec, y: vec) -> bvec", "Component-wise `<=`.";
    "greaterThan", "greaterThan(x: vec, y: vec) -> bvec", "Component-wise `>`.";
    "greaterThanEqual", "greaterThanEqual(x: vec, y: vec) -> bvec", "Component-wise `>=`.";
    "equal", "equal(x: vec, y: vec) -> bvec", "Component-wise `==`.";
    "notEqual", "notEqual(x: vec, y: vec) -> bvec", "Component-wise `!=`.";
    "any", "any(x: bvec) -> bool", "True when any component is true.";
    "all", "all(x: bvec) -> bool", "True when every component is true.";
    "not", "not(x: bvec) -> bvec", "Component-wise logical negation.";

    // ── Integer ───────────────────────────────────────────────────────────
    "uaddCarry", "uaddCarry(x: genUType, y: genUType, out carry: genUType) -> genUType", "Addition with the carry bit reported.";
    "usubBorrow", "usubBorrow(x: genUType, y: genUType, out borrow: genUType) -> genUType", "Subtraction with the borrow bit reported.";
    "umulExtended", "umulExtended(x: genUType, y: genUType, out msb: genUType, out lsb: genUType)", "Full 64-bit unsigned product.";
    "imulExtended", "imulExtended(x: genIType, y: genIType, out msb: genIType, out lsb: genIType)", "Full 64-bit signed product.";
    "bitfieldExtract", "bitfieldExtract(value: genIType, offset: int, bits: int) -> genIType", "Bit field extracted from each component.";
    "bitfieldInsert", "bitfieldInsert(base: genIType, insert: genIType, offset: int, bits: int) -> genIType", "Bit field replaced in each component.";
    "bitfieldReverse", "bitfieldReverse(value: genIType) -> genIType", "Bits of each component reversed.";
    "bitCount", "bitCount(value: genIType) -> genIType", "Set bits in each component (population count).";
    "findLSB", "findLSB(value: genIType) -> genIType", "Index of the lowest set bit, or -1.";
    "findMSB", "findMSB(value: genIType) -> genIType", "Index of the highest set bit, or -1.";

    // ── Texture ───────────────────────────────────────────────────────────
    "textureSize", "textureSize(sampler: gsampler, lod: int) -> ivecN", "Size of a mip level, in texels.";
    "textureQueryLod", "textureQueryLod(sampler: gsampler, P: vec) -> vec2", "Mip level that would be sampled.";
    "textureQueryLevels", "textureQueryLevels(sampler: gsampler) -> int", "Mip level count.";
    "textureSamples", "textureSamples(sampler: gsampler2DMS) -> int", "Sample count of a multisampled texture.";
    "texture", "texture(sampler: gsampler, P: vec, bias: float) -> gvec4", "Filtered sample.";
    "textureProj", "textureProj(sampler: gsampler, P: vec, bias: float) -> gvec4", "Sample with the coordinates divided by their last component.";
    "textureLod", "textureLod(sampler: gsampler, P: vec, lod: float) -> gvec4", "Sample at an explicit mip level.";
    "textureOffset", "textureOffset(sampler: gsampler, P: vec, offset: ivec, bias: float) -> gvec4", "Sample with a constant texel offset.";
    "texelFetch", "texelFetch(sampler: gsampler, P: ivec, lod: int) -> gvec4", "Unfiltered read of a single texel.";
    "texelFetchOffset", "texelFetchOffset(sampler: gsampler, P: ivec, lod: int, offset: ivec) -> gvec4", "Unfiltered read with a constant texel offset.";
    "textureProjOffset", "textureProjOffset(sampler: gsampler, P: vec, offset: ivec, bias: float) -> gvec4", "Projective sample with a texel offset.";
    "textureLodOffset", "textureLodOffset(sampler: gsampler, P: vec, lod: float, offset: ivec) -> gvec4", "Explicit-lod sample with a texel offset.";
    "textureProjLod", "textureProjLod(sampler: gsampler, P: vec, lod: float) -> gvec4", "Projective sample at an explicit mip level.";
    "textureProjLodOffset", "textureProjLodOffset(sampler: gsampler, P: vec, lod: float, offset: ivec) -> gvec4", "Projective explicit-lod sample with a texel offset.";
    "textureGrad", "textureGrad(sampler: gsampler, P: vec, dPdx: vec, dPdy: vec) -> gvec4", "Sample with explicit derivatives.";
    "textureGradOffset", "textureGradOffset(sampler: gsampler, P: vec, dPdx: vec, dPdy: vec, offset: ivec) -> gvec4", "Explicit-gradient sample with a texel offset.";
    "textureProjGrad", "textureProjGrad(sampler: gsampler, P: vec, dPdx: vec, dPdy: vec) -> gvec4", "Projective sample with explicit derivatives.";
    "textureProjGradOffset", "textureProjGradOffset(sampler: gsampler, P: vec, dPdx: vec, dPdy: vec, offset: ivec) -> gvec4", "Projective explicit-gradient sample with a texel offset.";
    "textureGather", "textureGather(sampler: gsampler2D, P: vec2, comp: int) -> gvec4", "One component from each of the four sampled texels.";
    "textureGatherOffset", "textureGatherOffset(sampler: gsampler2D, P: vec2, offset: ivec2, comp: int) -> gvec4", "Gather with a texel offset.";
    "textureGatherOffsets", "textureGatherOffsets(sampler: gsampler2D, P: vec2, offsets: ivec2[4], comp: int) -> gvec4", "Gather with four independent texel offsets.";

    // ── Atomic counters and atomics ───────────────────────────────────────
    "atomicCounterIncrement", "atomicCounterIncrement(c: atomic_uint) -> uint", "Increments the counter, returning the previous value.";
    "atomicCounterDecrement", "atomicCounterDecrement(c: atomic_uint) -> uint", "Decrements the counter, returning the new value.";
    "atomicCounter", "atomicCounter(c: atomic_uint) -> uint", "Reads the counter.";
    "atomicAdd", "atomicAdd(inout mem: uint, data: uint) -> uint", "Atomic add, returning the previous value.";
    "atomicMin", "atomicMin(inout mem: uint, data: uint) -> uint", "Atomic minimum, returning the previous value.";
    "atomicMax", "atomicMax(inout mem: uint, data: uint) -> uint", "Atomic maximum, returning the previous value.";
    "atomicAnd", "atomicAnd(inout mem: uint, data: uint) -> uint", "Atomic bitwise and, returning the previous value.";
    "atomicOr", "atomicOr(inout mem: uint, data: uint) -> uint", "Atomic bitwise or, returning the previous value.";
    "atomicXor", "atomicXor(inout mem: uint, data: uint) -> uint", "Atomic bitwise xor, returning the previous value.";
    "atomicExchange", "atomicExchange(inout mem: uint, data: uint) -> uint", "Atomic swap, returning the previous value.";
    "atomicCompSwap", "atomicCompSwap(inout mem: uint, compare: uint, data: uint) -> uint", "Compare-and-swap, returning the previous value.";

    // ── Images ────────────────────────────────────────────────────────────
    "imageSize", "imageSize(image: gimage) -> ivecN", "Size of the image, in texels.";
    "imageSamples", "imageSamples(image: gimage2DMS) -> int", "Sample count of a multisampled image.";
    "imageLoad", "imageLoad(image: gimage, P: ivec) -> gvec4", "Reads a single texel.";
    "imageStore", "imageStore(image: gimage, P: ivec, data: gvec4)", "Writes a single texel.";
    "imageAtomicAdd", "imageAtomicAdd(image: gimage, P: ivec, data: uint) -> uint", "Atomic add on a texel.";
    "imageAtomicMin", "imageAtomicMin(image: gimage, P: ivec, data: uint) -> uint", "Atomic minimum on a texel.";
    "imageAtomicMax", "imageAtomicMax(image: gimage, P: ivec, data: uint) -> uint", "Atomic maximum on a texel.";
    "imageAtomicAnd", "imageAtomicAnd(image: gimage, P: ivec, data: uint) -> uint", "Atomic bitwise and on a texel.";
    "imageAtomicOr", "imageAtomicOr(image: gimage, P: ivec, data: uint) -> uint", "Atomic bitwise or on a texel.";
    "imageAtomicXor", "imageAtomicXor(image: gimage, P: ivec, data: uint) -> uint", "Atomic bitwise xor on a texel.";
    "imageAtomicExchange", "imageAtomicExchange(image: gimage, P: ivec, data: uint) -> uint", "Atomic swap on a texel.";
    "imageAtomicCompSwap", "imageAtomicCompSwap(image: gimage, P: ivec, compare: uint, data: uint) -> uint", "Compare-and-swap on a texel.";
    "subpassLoad", "subpassLoad(subpass: gsubpassInput) -> gvec4", "Reads the current fragment from an input attachment. Vulkan only.";

    // ── Fragment processing ───────────────────────────────────────────────
    "dFdx", "dFdx(p: genType) -> genType", "Partial derivative along x. Fragment stage only.";
    "dFdy", "dFdy(p: genType) -> genType", "Partial derivative along y. Fragment stage only.";
    "dFdxFine", "dFdxFine(p: genType) -> genType", "Fine partial derivative along x.";
    "dFdyFine", "dFdyFine(p: genType) -> genType", "Fine partial derivative along y.";
    "dFdxCoarse", "dFdxCoarse(p: genType) -> genType", "Coarse partial derivative along x.";
    "dFdyCoarse", "dFdyCoarse(p: genType) -> genType", "Coarse partial derivative along y.";
    "fwidth", "fwidth(p: genType) -> genType", "abs(dFdx(p)) + abs(dFdy(p)).";
    "fwidthFine", "fwidthFine(p: genType) -> genType", "Fine filter width.";
    "fwidthCoarse", "fwidthCoarse(p: genType) -> genType", "Coarse filter width.";
    "interpolateAtCentroid", "interpolateAtCentroid(interpolant: genType) -> genType", "Re-interpolates at the centroid of the covered area.";
    "interpolateAtSample", "interpolateAtSample(interpolant: genType, sample: int) -> genType", "Re-interpolates at a given sample position.";
    "interpolateAtOffset", "interpolateAtOffset(interpolant: genType, offset: vec2) -> genType", "Re-interpolates at a pixel-relative offset.";

    // ── Synchronisation ───────────────────────────────────────────────────
    "barrier", "barrier()", "Execution barrier across the workgroup.";
    "memoryBarrier", "memoryBarrier()", "Orders every memory access.";
    "memoryBarrierAtomicCounter", "memoryBarrierAtomicCounter()", "Orders atomic-counter accesses.";
    "memoryBarrierBuffer", "memoryBarrierBuffer()", "Orders buffer accesses.";
    "memoryBarrierShared", "memoryBarrierShared()", "Orders shared-variable accesses. Compute stage only.";
    "memoryBarrierImage", "memoryBarrierImage()", "Orders image accesses.";
    "groupMemoryBarrier", "groupMemoryBarrier()", "Orders memory accesses within the workgroup.";

    // ── Geometry shader ───────────────────────────────────────────────────
    "EmitVertex", "EmitVertex()", "Emits the current output vertex.";
    "EndPrimitive", "EndPrimitive()", "Ends the current output primitive.";
    "EmitStreamVertex", "EmitStreamVertex(stream: int)", "Emits the current vertex to a given stream.";
    "EndStreamPrimitive", "EndStreamPrimitive(stream: int)", "Ends the current primitive on a given stream.";
];

/// Types spelled out in full. The opaque families are generated; see
/// [`opaque_type_names`].
pub const TYPES: &[&str] = &[
    "void", "bool", "int", "uint", "float", "double", "atomic_uint",
    "vec2", "vec3", "vec4",
    "bvec2", "bvec3", "bvec4",
    "ivec2", "ivec3", "ivec4",
    "uvec2", "uvec3", "uvec4",
    "dvec2", "dvec3", "dvec4",
    "mat2", "mat3", "mat4",
    "mat2x2", "mat2x3", "mat2x4", "mat3x2", "mat3x3", "mat3x4",
    "mat4x2", "mat4x3", "mat4x4",
    "dmat2", "dmat3", "dmat4",
    "dmat2x2", "dmat2x3", "dmat2x4", "dmat3x2", "dmat3x3", "dmat3x4",
    "dmat4x2", "dmat4x3", "dmat4x4",
    "sampler", "samplerShadow", "subpassInput", "subpassInputMS",
];

pub const KEYWORDS: &[&str] = &[
    "attribute", "break", "buffer", "case", "centroid", "coherent", "const",
    "continue", "default", "discard", "do", "else", "false", "flat", "for",
    "highp", "if", "in", "inout", "invariant", "layout", "lowp", "mediump",
    "noperspective", "out", "patch", "precise", "precision", "readonly",
    "restrict", "return", "sample", "shared", "smooth", "struct", "subroutine",
    "switch", "true", "uniform", "varying", "volatile", "while", "writeonly",
];

/// Names valid inside `layout(…)`.
pub const LAYOUT_QUALIFIERS: &[&str] = &[
    "location", "binding", "set", "offset", "align", "component", "index",
    "input_attachment_index", "push_constant", "constant_id",
    "std140", "std430", "packed", "shared", "row_major", "column_major",
    "local_size_x", "local_size_y", "local_size_z",
    "points", "lines", "lines_adjacency", "triangles", "triangles_adjacency",
    "line_strip", "triangle_strip", "max_vertices", "invocations", "stream",
    "vertices", "quads", "isolines", "equal_spacing",
    "fractional_even_spacing", "fractional_odd_spacing", "cw", "ccw",
    "point_mode", "origin_upper_left", "pixel_center_integer",
    "early_fragment_tests", "depth_any", "depth_greater", "depth_less",
    "depth_unchanged",
];

/// Directive names, without the `#`.
pub const DIRECTIVES: &[&str] = &[
    "version", "define", "undef", "if", "ifdef", "ifndef", "else", "elif",
    "endif", "error", "pragma", "extension", "line", "include",
];

/// The `gl_`-prefixed predeclared variables. The doc names the stages each
/// belongs to, which is the question a reader actually has about them.
pub const VARIABLES: &[Builtin] = builtins![
    "gl_Position", "out vec4 gl_Position", "Clip-space vertex position. Vertex, tessellation and geometry stages.";
    "gl_PointSize", "out float gl_PointSize", "Rasterised point diameter. Vertex, tessellation and geometry stages.";
    "gl_VertexID", "in int gl_VertexID", "Index of the current vertex. Vertex stage, OpenGL.";
    "gl_InstanceID", "in int gl_InstanceID", "Index of the current instance. Vertex stage, OpenGL.";
    "gl_VertexIndex", "in int gl_VertexIndex", "Index of the current vertex. Vertex stage, Vulkan.";
    "gl_InstanceIndex", "in int gl_InstanceIndex", "Index of the current instance. Vertex stage, Vulkan.";
    "gl_DrawID", "in int gl_DrawID", "Index of the draw within a multi-draw. Vertex stage.";
    "gl_BaseVertex", "in int gl_BaseVertex", "Base vertex of the current draw. Vertex stage.";
    "gl_BaseInstance", "in int gl_BaseInstance", "Base instance of the current draw. Vertex stage.";
    "gl_ClipDistance", "float gl_ClipDistance[]", "User clip distances. Vertex and fragment stages.";
    "gl_CullDistance", "float gl_CullDistance[]", "User cull distances. Vertex and fragment stages.";
    "gl_FragCoord", "in vec4 gl_FragCoord", "Window-space fragment position. Fragment stage.";
    "gl_FrontFacing", "in bool gl_FrontFacing", "Whether the fragment is front-facing. Fragment stage.";
    "gl_PointCoord", "in vec2 gl_PointCoord", "Position within a rasterised point. Fragment stage.";
    "gl_FragDepth", "out float gl_FragDepth", "Depth written by the shader. Fragment stage.";
    "gl_SampleID", "in int gl_SampleID", "Sample number of the current fragment. Fragment stage.";
    "gl_SamplePosition", "in vec2 gl_SamplePosition", "Position of the current sample. Fragment stage.";
    "gl_SampleMask", "out int gl_SampleMask[]", "Coverage mask written by the shader. Fragment stage.";
    "gl_SampleMaskIn", "in int gl_SampleMaskIn[]", "Incoming coverage mask. Fragment stage.";
    "gl_PrimitiveID", "int gl_PrimitiveID", "Index of the current primitive. Fragment and geometry stages.";
    "gl_Layer", "int gl_Layer", "Layer of a layered framebuffer. Fragment and geometry stages.";
    "gl_ViewportIndex", "int gl_ViewportIndex", "Viewport index. Fragment and geometry stages.";
    "gl_FragColor", "out vec4 gl_FragColor", "Fragment colour. Legacy: removed in core profiles above 1.30.";
    "gl_FragData", "out vec4 gl_FragData[]", "Fragment colours per attachment. Legacy.";
    "gl_NumWorkGroups", "in uvec3 gl_NumWorkGroups", "Dispatch size in workgroups. Compute stage.";
    "gl_WorkGroupSize", "const uvec3 gl_WorkGroupSize", "Workgroup dimensions declared by `layout(local_size_x = …)`. Compute stage.";
    "gl_WorkGroupID", "in uvec3 gl_WorkGroupID", "Workgroup coordinates within the dispatch. Compute stage.";
    "gl_LocalInvocationID", "in uvec3 gl_LocalInvocationID", "Invocation coordinates within the workgroup. Compute stage.";
    "gl_GlobalInvocationID", "in uvec3 gl_GlobalInvocationID", "Invocation coordinates within the dispatch. Compute stage.";
    "gl_LocalInvocationIndex", "in uint gl_LocalInvocationIndex", "Linearised local invocation id. Compute stage.";
    "gl_in", "in gl_PerVertex gl_in[]", "Per-vertex inputs. Geometry and tessellation stages.";
    "gl_out", "out gl_PerVertex gl_out[]", "Per-vertex outputs. Tessellation control stage.";
    "gl_InvocationID", "in int gl_InvocationID", "Invocation number. Geometry and tessellation control stages.";
    "gl_PatchVerticesIn", "in int gl_PatchVerticesIn", "Vertices per input patch. Tessellation stages.";
    "gl_TessCoord", "in vec3 gl_TessCoord", "Position within the tessellated primitive. Tessellation evaluation stage.";
    "gl_TessLevelOuter", "patch out float gl_TessLevelOuter[4]", "Outer tessellation levels. Tessellation stages.";
    "gl_TessLevelInner", "patch out float gl_TessLevelInner[2]", "Inner tessellation levels. Tessellation stages.";
];

/// The shapes an opaque type can take: `sampler2D`, `image2DMSArray`, …
const SHAPES: &[&str] = &[
    "1D", "2D", "3D", "Cube", "2DRect", "1DArray", "2DArray", "CubeArray",
    "Buffer", "2DMS", "2DMSArray",
];

/// The shapes a depth-comparison sampler can take. There is no `sampler3DShadow`.
const SHADOW_SHAPES: &[&str] =
    &["1D", "2D", "Cube", "2DRect", "1DArray", "2DArray", "CubeArray"];

const BASES: &[&str] = &["sampler", "image", "texture"];

/// Whether `name` is one of the generated opaque type names.
///
/// Decomposed rather than looked up so the check stays allocation-free — the
/// lexer calls it for every word in the file.
pub fn is_opaque_type(name: &str) -> bool {
    // `usampler2D` and `image2D` both have to work, and stripping the leading
    // `i` from the latter would leave `mage2D`, so try the unprefixed reading
    // first and only then the prefixed one.
    if base_and_shape(name, true) {
        return true;
    }
    match name.as_bytes().first() {
        Some(b'i' | b'u') => base_and_shape(&name[1..], false),
        _ => false,
    }
}

/// Splits a base off `name` and checks what remains is a shape.
///
/// `shadow` is false for the `i`/`u`-prefixed reading: depth-comparison
/// samplers return floats, so there is no `isamplerCubeShadow`.
fn base_and_shape(name: &str, shadow: bool) -> bool {
    let Some(base) = BASES.iter().find(|base| name.starts_with(**base)) else {
        return false;
    };
    let rest = &name[base.len()..];
    if let Some(shape) = rest.strip_suffix("Shadow") {
        return shadow && *base == "sampler" && SHADOW_SHAPES.contains(&shape);
    }
    SHAPES.contains(&rest)
}

/// Every opaque type name, for completion.
///
/// Generated once and leaked so the result is `&'static`: there are 106 of
/// them, they never change, and every caller downstream wants `&'static str`.
pub fn opaque_type_names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = Vec::with_capacity(BASES.len() * SHAPES.len() * 3 + SHADOW_SHAPES.len());
        for base in BASES {
            for prefix in ["", "i", "u"] {
                for shape in SHAPES {
                    names.push(&*Box::leak(format!("{prefix}{base}{shape}").into_boxed_str()));
                }
            }
        }
        for shape in SHADOW_SHAPES {
            names.push(&*Box::leak(format!("sampler{shape}Shadow").into_boxed_str()));
        }
        names
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_types_decompose() {
        for name in [
            "sampler2D",
            "usampler2D",
            "isampler2DArray",
            "image3D",
            "uimage2DMSArray",
            "texture2D",
            "samplerCubeShadow",
            "sampler2DRectShadow",
        ] {
            assert!(is_opaque_type(name), "{name} should be an opaque type");
        }
    }

    #[test]
    fn near_misses_are_not_opaque_types() {
        for name in [
            "sampler4D",
            "sampler3DShadow", // no such thing: 3D has no depth-comparison form
            "isamplerCubeShadow", // shadow samplers take no i/u prefix
            "myImage2D",
            "image",
            "samplerize",
        ] {
            assert!(!is_opaque_type(name), "{name} should not be an opaque type");
        }
    }

    /// The generated list and the predicate must agree, or completion offers a
    /// name the lexer then paints as an ordinary identifier.
    #[test]
    fn the_generated_list_agrees_with_the_predicate() {
        for name in opaque_type_names() {
            assert!(is_opaque_type(name), "{name} is listed but not recognised");
        }
        assert_eq!(opaque_type_names().len(), 3 * 11 * 3 + 7);
    }

    #[test]
    fn plain_sampler_names_stay_in_the_explicit_list() {
        // `sampler` and `samplerShadow` have no shape, so the predicate must
        // not claim them — `TYPES` does.
        assert!(!is_opaque_type("sampler"));
        assert!(TYPES.contains(&"sampler"));
        assert!(TYPES.contains(&"samplerShadow"));
    }
}
