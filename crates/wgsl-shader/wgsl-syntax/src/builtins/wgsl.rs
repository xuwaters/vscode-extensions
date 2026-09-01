//! WGSL's predeclared names.
//!
//! Signatures follow the spec's notation: `T` is a scalar, `vecN<T>` any vector
//! width, `AS` an address space. They are read by a human in a hover popup, not
//! by a type checker — naga does the checking.

use super::{Builtin, builtins};

pub const FUNCTIONS: &[Builtin] = builtins![
    // ── Numeric ───────────────────────────────────────────────────────────
    "abs", "abs(e: T) -> T", "Absolute value.";
    "acos", "acos(e: T) -> T", "Arc cosine, in radians.";
    "acosh", "acosh(e: T) -> T", "Inverse hyperbolic cosine.";
    "asin", "asin(e: T) -> T", "Arc sine, in radians.";
    "asinh", "asinh(e: T) -> T", "Inverse hyperbolic sine.";
    "atan", "atan(e: T) -> T", "Arc tangent, in radians.";
    "atanh", "atanh(e: T) -> T", "Inverse hyperbolic tangent.";
    "atan2", "atan2(y: T, x: T) -> T", "Arc tangent of y/x, using the signs of both to pick the quadrant.";
    "ceil", "ceil(e: T) -> T", "Nearest integer at or above e.";
    "clamp", "clamp(e: T, low: T, high: T) -> T", "e constrained to [low, high].";
    "cos", "cos(e: T) -> T", "Cosine of an angle in radians.";
    "cosh", "cosh(e: T) -> T", "Hyperbolic cosine.";
    "countLeadingZeros", "countLeadingZeros(e: T) -> T", "Leading zero bits in each component.";
    "countOneBits", "countOneBits(e: T) -> T", "Set bits in each component (population count).";
    "countTrailingZeros", "countTrailingZeros(e: T) -> T", "Trailing zero bits in each component.";
    "cross", "cross(a: vec3<f32>, b: vec3<f32>) -> vec3<f32>", "Cross product.";
    "degrees", "degrees(e: T) -> T", "Radians converted to degrees.";
    "determinant", "determinant(m: matCxC<T>) -> T", "Determinant of a square matrix.";
    "distance", "distance(a: T, b: T) -> f32", "Euclidean distance between two points.";
    "dot", "dot(a: vecN<T>, b: vecN<T>) -> T", "Dot product.";
    "dot4U8Packed", "dot4U8Packed(a: u32, b: u32) -> u32", "Dot product of two packed vec4<u8> values.";
    "dot4I8Packed", "dot4I8Packed(a: u32, b: u32) -> i32", "Dot product of two packed vec4<i8> values.";
    "exp", "exp(e: T) -> T", "e raised to the power of the argument.";
    "exp2", "exp2(e: T) -> T", "2 raised to the power of the argument.";
    "extractBits", "extractBits(e: T, offset: u32, count: u32) -> T", "Bit field extracted from each component.";
    "faceForward", "faceForward(e1: T, e2: T, e3: T) -> T", "e1 flipped to face away from e3.";
    "firstLeadingBit", "firstLeadingBit(e: T) -> T", "Index of the highest set bit, or -1/0xffffffff.";
    "firstTrailingBit", "firstTrailingBit(e: T) -> T", "Index of the lowest set bit, or -1/0xffffffff.";
    "floor", "floor(e: T) -> T", "Nearest integer at or below e.";
    "fma", "fma(e1: T, e2: T, e3: T) -> T", "Fused multiply-add: e1 * e2 + e3, rounded once.";
    "fract", "fract(e: T) -> T", "Fractional part: e - floor(e).";
    "frexp", "frexp(e: T) -> __frexp_result", "Splits e into a fraction and an exponent.";
    "insertBits", "insertBits(e: T, newbits: T, offset: u32, count: u32) -> T", "e with a bit field replaced.";
    "inverseSqrt", "inverseSqrt(e: T) -> T", "1 / sqrt(e).";
    "ldexp", "ldexp(e1: T, e2: I) -> T", "e1 * 2^e2.";
    "length", "length(e: T) -> f32", "Vector length.";
    "log", "log(e: T) -> T", "Natural logarithm.";
    "log2", "log2(e: T) -> T", "Base-2 logarithm.";
    "max", "max(e1: T, e2: T) -> T", "Component-wise maximum.";
    "min", "min(e1: T, e2: T) -> T", "Component-wise minimum.";
    "mix", "mix(e1: T, e2: T, e3: T) -> T", "Linear blend: e1 * (1 - e3) + e2 * e3.";
    "modf", "modf(e: T) -> __modf_result", "Splits e into fractional and whole parts.";
    "normalize", "normalize(e: vecN<T>) -> vecN<T>", "Unit vector in the same direction.";
    "pow", "pow(e1: T, e2: T) -> T", "e1 raised to the power e2.";
    "quantizeToF16", "quantizeToF16(e: T) -> T", "Value quantized to f16 precision, kept as f32.";
    "radians", "radians(e: T) -> T", "Degrees converted to radians.";
    "reflect", "reflect(e1: T, e2: T) -> T", "Incident vector reflected about a normal.";
    "refract", "refract(e1: T, e2: T, e3: f32) -> T", "Refraction vector for a ratio of indices.";
    "reverseBits", "reverseBits(e: T) -> T", "Bits of each component reversed.";
    "round", "round(e: T) -> T", "Nearest integer, halfway cases to even.";
    "saturate", "saturate(e: T) -> T", "clamp(e, 0.0, 1.0).";
    "sign", "sign(e: T) -> T", "-1, 0 or +1 by the sign of each component.";
    "sin", "sin(e: T) -> T", "Sine of an angle in radians.";
    "sinh", "sinh(e: T) -> T", "Hyperbolic sine.";
    "smoothstep", "smoothstep(low: T, high: T, x: T) -> T", "Hermite interpolation between two edges.";
    "sqrt", "sqrt(e: T) -> T", "Square root.";
    "step", "step(edge: T, x: T) -> T", "0.0 below the edge, 1.0 at or above it.";
    "tan", "tan(e: T) -> T", "Tangent of an angle in radians.";
    "tanh", "tanh(e: T) -> T", "Hyperbolic tangent.";
    "transpose", "transpose(m: matCxR<T>) -> matRxC<T>", "Matrix transpose.";
    "trunc", "trunc(e: T) -> T", "Fractional part discarded.";

    // ── Logical ───────────────────────────────────────────────────────────
    "all", "all(e: vecN<bool>) -> bool", "True when every component is true.";
    "any", "any(e: vecN<bool>) -> bool", "True when any component is true.";
    "select", "select(f: T, t: T, cond: bool) -> T", "t when cond is true, otherwise f.";

    // ── Derivatives (fragment stage only) ─────────────────────────────────
    "dpdx", "dpdx(e: T) -> T", "Partial derivative along x. Fragment stage only.";
    "dpdxCoarse", "dpdxCoarse(e: T) -> T", "Coarse partial derivative along x.";
    "dpdxFine", "dpdxFine(e: T) -> T", "Fine partial derivative along x.";
    "dpdy", "dpdy(e: T) -> T", "Partial derivative along y. Fragment stage only.";
    "dpdyCoarse", "dpdyCoarse(e: T) -> T", "Coarse partial derivative along y.";
    "dpdyFine", "dpdyFine(e: T) -> T", "Fine partial derivative along y.";
    "fwidth", "fwidth(e: T) -> T", "abs(dpdx(e)) + abs(dpdy(e)).";
    "fwidthCoarse", "fwidthCoarse(e: T) -> T", "Coarse filter width.";
    "fwidthFine", "fwidthFine(e: T) -> T", "Fine filter width.";

    // ── Texture ───────────────────────────────────────────────────────────
    "textureDimensions", "textureDimensions(t: texture, level: u32 = 0) -> vecN<u32>", "Size of a mip level, in texels.";
    "textureGather", "textureGather(component: i32, t: texture_2d<T>, s: sampler, coords: vec2<f32>) -> vec4<T>", "One component from each of the four sampled texels.";
    "textureGatherCompare", "textureGatherCompare(t: texture_depth_2d, s: sampler_comparison, coords: vec2<f32>, depth_ref: f32) -> vec4<f32>", "Depth comparison across the four sampled texels.";
    "textureLoad", "textureLoad(t: texture, coords: vecN<I>, level: I) -> vec4<T>", "Unfiltered read of a single texel.";
    "textureNumLayers", "textureNumLayers(t: texture_2d_array<T>) -> u32", "Layer count of an array texture.";
    "textureNumLevels", "textureNumLevels(t: texture) -> u32", "Mip level count.";
    "textureNumSamples", "textureNumSamples(t: texture_multisampled_2d<T>) -> u32", "Sample count of a multisampled texture.";
    "textureSample", "textureSample(t: texture_2d<f32>, s: sampler, coords: vec2<f32>) -> vec4<f32>", "Filtered sample. Fragment stage only — it needs derivatives.";
    "textureSampleBaseClampToEdge", "textureSampleBaseClampToEdge(t: texture_2d<f32>, s: sampler, coords: vec2<f32>) -> vec4<f32>", "Samples mip level 0 with coordinates clamped to the edge.";
    "textureSampleBias", "textureSampleBias(t: texture_2d<f32>, s: sampler, coords: vec2<f32>, bias: f32) -> vec4<f32>", "Sample with a bias applied to the mip level.";
    "textureSampleCompare", "textureSampleCompare(t: texture_depth_2d, s: sampler_comparison, coords: vec2<f32>, depth_ref: f32) -> f32", "Filtered depth comparison. Fragment stage only.";
    "textureSampleCompareLevel", "textureSampleCompareLevel(t: texture_depth_2d, s: sampler_comparison, coords: vec2<f32>, depth_ref: f32) -> f32", "Depth comparison at mip level 0. Usable in any stage.";
    "textureSampleGrad", "textureSampleGrad(t: texture_2d<f32>, s: sampler, coords: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32>", "Sample with explicit derivatives.";
    "textureSampleLevel", "textureSampleLevel(t: texture_2d<f32>, s: sampler, coords: vec2<f32>, level: f32) -> vec4<f32>", "Sample at an explicit mip level.";
    "textureStore", "textureStore(t: texture_storage_2d<F, write>, coords: vec2<I>, value: vec4<T>)", "Write a single texel.";

    // ── Atomic ────────────────────────────────────────────────────────────
    "atomicLoad", "atomicLoad(p: ptr<AS, atomic<T>>) -> T", "Atomically read.";
    "atomicStore", "atomicStore(p: ptr<AS, atomic<T>>, v: T)", "Atomically write.";
    "atomicAdd", "atomicAdd(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic add, returning the previous value.";
    "atomicSub", "atomicSub(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic subtract, returning the previous value.";
    "atomicMax", "atomicMax(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic maximum, returning the previous value.";
    "atomicMin", "atomicMin(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic minimum, returning the previous value.";
    "atomicAnd", "atomicAnd(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic bitwise and, returning the previous value.";
    "atomicOr", "atomicOr(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic bitwise or, returning the previous value.";
    "atomicXor", "atomicXor(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic bitwise xor, returning the previous value.";
    "atomicExchange", "atomicExchange(p: ptr<AS, atomic<T>>, v: T) -> T", "Atomic swap, returning the previous value.";
    "atomicCompareExchangeWeak", "atomicCompareExchangeWeak(p: ptr<AS, atomic<T>>, cmp: T, v: T) -> __atomic_compare_exchange_result<T>", "Compare-and-swap. May fail spuriously.";

    // ── Data packing ──────────────────────────────────────────────────────
    "pack2x16float", "pack2x16float(e: vec2<f32>) -> u32", "Two f32 packed as two f16.";
    "pack2x16snorm", "pack2x16snorm(e: vec2<f32>) -> u32", "Two f32 packed as two signed normalised 16-bit ints.";
    "pack2x16unorm", "pack2x16unorm(e: vec2<f32>) -> u32", "Two f32 packed as two unsigned normalised 16-bit ints.";
    "pack4x8snorm", "pack4x8snorm(e: vec4<f32>) -> u32", "Four f32 packed as four signed normalised bytes.";
    "pack4x8unorm", "pack4x8unorm(e: vec4<f32>) -> u32", "Four f32 packed as four unsigned normalised bytes.";
    "pack4xI8", "pack4xI8(e: vec4<i32>) -> u32", "Low byte of each component packed into a u32.";
    "pack4xU8", "pack4xU8(e: vec4<u32>) -> u32", "Low byte of each component packed into a u32.";
    "unpack2x16float", "unpack2x16float(e: u32) -> vec2<f32>", "Two f16 unpacked to f32.";
    "unpack2x16snorm", "unpack2x16snorm(e: u32) -> vec2<f32>", "Two signed normalised 16-bit ints unpacked to f32.";
    "unpack2x16unorm", "unpack2x16unorm(e: u32) -> vec2<f32>", "Two unsigned normalised 16-bit ints unpacked to f32.";
    "unpack4x8snorm", "unpack4x8snorm(e: u32) -> vec4<f32>", "Four signed normalised bytes unpacked to f32.";
    "unpack4x8unorm", "unpack4x8unorm(e: u32) -> vec4<f32>", "Four unsigned normalised bytes unpacked to f32.";
    "unpack4xI8", "unpack4xI8(e: u32) -> vec4<i32>", "Four bytes sign-extended to i32.";
    "unpack4xU8", "unpack4xU8(e: u32) -> vec4<u32>", "Four bytes zero-extended to u32.";

    // ── Synchronisation (compute stage only) ──────────────────────────────
    "storageBarrier", "storageBarrier()", "Control and memory barrier over storage memory.";
    "textureBarrier", "textureBarrier()", "Control and memory barrier over texture memory.";
    "workgroupBarrier", "workgroupBarrier()", "Control and memory barrier over workgroup memory.";
    "workgroupUniformLoad", "workgroupUniformLoad(p: ptr<workgroup, T>) -> T", "Barrier plus a load broadcast to the whole workgroup.";

    // ── Conversion ────────────────────────────────────────────────────────
    "arrayLength", "arrayLength(p: ptr<storage, array<E>>) -> u32", "Element count of a runtime-sized array.";
    "bitcast", "bitcast<T>(e: S) -> T", "Reinterprets the bits as another type of the same width.";
];

pub const TYPES: &[&str] = &[
    "bool", "f16", "f32", "i32", "u32",
    "vec2", "vec3", "vec4",
    "vec2i", "vec3i", "vec4i", "vec2u", "vec3u", "vec4u",
    "vec2f", "vec3f", "vec4f", "vec2h", "vec3h", "vec4h",
    "mat2x2", "mat2x3", "mat2x4", "mat3x2", "mat3x3", "mat3x4",
    "mat4x2", "mat4x3", "mat4x4",
    "mat2x2f", "mat2x3f", "mat2x4f", "mat3x2f", "mat3x3f", "mat3x4f",
    "mat4x2f", "mat4x3f", "mat4x4f",
    "mat2x2h", "mat2x3h", "mat2x4h", "mat3x2h", "mat3x3h", "mat3x4h",
    "mat4x2h", "mat4x3h", "mat4x4h",
    "array", "atomic", "ptr",
    "sampler", "sampler_comparison",
    "texture_1d", "texture_2d", "texture_2d_array", "texture_3d",
    "texture_cube", "texture_cube_array", "texture_multisampled_2d",
    "texture_storage_1d", "texture_storage_2d", "texture_storage_2d_array",
    "texture_storage_3d", "texture_depth_2d", "texture_depth_2d_array",
    "texture_depth_cube", "texture_depth_cube_array",
    "texture_depth_multisampled_2d", "texture_external",
];

pub const KEYWORDS: &[&str] = &[
    "alias", "break", "case", "const", "const_assert", "continue", "continuing",
    "default", "diagnostic", "discard", "else", "enable", "false", "fn", "for",
    "if", "let", "loop", "override", "requires", "return", "struct", "switch",
    "true", "var", "while",
];

/// Names valid after `@`.
pub const ATTRIBUTES: &[Builtin] = builtins![
    "align", "@align(n)", "Byte alignment of a struct member.";
    "binding", "@binding(n)", "Binding number within its group.";
    "blend_src", "@blend_src(n)", "Dual-source blending input index.";
    "builtin", "@builtin(name)", "Marks the value as a pipeline built-in.";
    "compute", "@compute", "Marks the function as a compute entry point.";
    "const", "@const", "Marks the function as usable in const expressions.";
    "diagnostic", "@diagnostic(severity, rule)", "Adjusts a diagnostic's severity for this range.";
    "fragment", "@fragment", "Marks the function as a fragment entry point.";
    "group", "@group(n)", "Bind group number.";
    "id", "@id(n)", "Pipeline-overridable constant id.";
    "interpolate", "@interpolate(type, sampling)", "How a value is interpolated between vertices.";
    "invariant", "@invariant", "The position must be computed identically across pipelines.";
    "location", "@location(n)", "IO location for a shader input or output.";
    "must_use", "@must_use", "The return value may not be discarded.";
    "size", "@size(n)", "Byte size reserved for a struct member.";
    "vertex", "@vertex", "Marks the function as a vertex entry point.";
    "workgroup_size", "@workgroup_size(x, y, z)", "Workgroup dimensions of a compute entry point.";
];

/// Names valid inside `@builtin(…)`.
pub const BUILTIN_VALUES: &[Builtin] = builtins![
    "vertex_index", "@builtin(vertex_index) : u32", "Index of the current vertex. Vertex input.";
    "instance_index", "@builtin(instance_index) : u32", "Index of the current instance. Vertex input.";
    "clip_distances", "@builtin(clip_distances) : array<f32, N>", "User-defined clip distances. Vertex output.";
    "position", "@builtin(position) : vec4<f32>", "Clip position out of a vertex shader; framebuffer position into a fragment shader.";
    "front_facing", "@builtin(front_facing) : bool", "Whether the fragment is front-facing. Fragment input.";
    "frag_depth", "@builtin(frag_depth) : f32", "Depth written by the fragment shader. Fragment output.";
    "sample_index", "@builtin(sample_index) : u32", "Sample number of the current fragment. Fragment input.";
    "sample_mask", "@builtin(sample_mask) : u32", "Coverage mask. Fragment input and output.";
    "local_invocation_id", "@builtin(local_invocation_id) : vec3<u32>", "Invocation coordinates within the workgroup. Compute input.";
    "local_invocation_index", "@builtin(local_invocation_index) : u32", "Linearised local invocation id. Compute input.";
    "global_invocation_id", "@builtin(global_invocation_id) : vec3<u32>", "Invocation coordinates within the dispatch. Compute input.";
    "workgroup_id", "@builtin(workgroup_id) : vec3<u32>", "Workgroup coordinates within the dispatch. Compute input.";
    "num_workgroups", "@builtin(num_workgroups) : vec3<u32>", "Dispatch size in workgroups. Compute input.";
    "subgroup_invocation_id", "@builtin(subgroup_invocation_id) : u32", "Invocation index within the subgroup.";
    "subgroup_size", "@builtin(subgroup_size) : u32", "Number of invocations in the subgroup.";
];

/// Address spaces, valid inside `var<…>`.
pub const ADDRESS_SPACES: &[Builtin] = builtins![
    "function", "var<function>", "Per-invocation. The default inside a function.";
    "private", "var<private>", "Per-invocation, module scope.";
    "workgroup", "var<workgroup>", "Shared by every invocation in a workgroup.";
    "uniform", "var<uniform>", "Read-only buffer, uniform across the draw.";
    "storage", "var<storage, read | read_write>", "Storage buffer.";
];

/// Access modes, the second argument to `var<storage, …>`.
pub const ACCESS_MODES: &[&str] = &["read", "write", "read_write"];

/// Arguments to `@interpolate(…)`: the type, then the optional sampling.
pub const INTERPOLATE_ARGS: &[&str] = &[
    "perspective", "linear", "flat", "center", "centroid", "sample", "first", "either",
];

/// Extensions namable in an `enable` directive.
pub const ENABLE_EXTENSIONS: &[&str] =
    &["f16", "clip_distances", "dual_source_blending", "subgroups"];

/// Features namable in a `requires` directive.
pub const LANGUAGE_FEATURES: &[&str] = &[
    "readonly_and_readwrite_storage_textures",
    "packed_4x8_integer_dot_product",
    "unrestricted_pointer_parameters",
    "pointer_composite_access",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_signatures_start_with_an_at_sign() {
        for attribute in ATTRIBUTES {
            assert!(
                attribute.signature.starts_with('@'),
                "{}: {}",
                attribute.name,
                attribute.signature
            );
        }
    }

    /// The old `src/shaderData.ts` list had no derivative functions, so
    /// `dpdx` never completed. Guard the gap it left.
    #[test]
    fn derivatives_and_logicals_are_present() {
        for name in ["dpdx", "dpdyFine", "fwidth", "all", "any", "select"] {
            assert!(
                FUNCTIONS.iter().any(|b| b.name == name),
                "{name} is missing"
            );
        }
    }
}
