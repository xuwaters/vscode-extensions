//! Types, as naga knows them and as a reader wants to see them.
//!
//! Two jobs. **Rendering**: turning a `TypeInner` into the spelling the user
//! writes, which differs per language — naga's one `Vector { size: Tri, scalar:
//! f32 }` is `vec3<f32>` in WGSL and `vec3` in GLSL. **Lookup**: finding the
//! type of the name under the cursor, and walking a `a.b.c` chain through it,
//! which is what makes member completion and a typed hover possible.
//!
//! Lookup is deliberately name-based rather than position-based. naga's IR has
//! spans, but they point at expressions, and the cursor is on an *identifier* —
//! going through the name is both simpler and more robust to the IR being one
//! keystroke out of date.

use naga::proc::{ResolveContext, ResolveError, TypeResolution};
use naga::{
    AddressSpace, ArraySize, Function, Handle, ImageClass, ImageDimension, Module, Scalar,
    ScalarKind, Type, TypeInner, VectorSize,
};
use wgsl_syntax::Language;

/// One name reachable through `.` from a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// The member's type, rendered.
    pub type_name: String,
    /// A note for the completion list: what kind of member this is.
    pub detail: &'static str,
}

/// Render a type by handle, preferring the name it was declared under.
pub fn render_handle(module: &Module, handle: Handle<Type>, language: Language) -> String {
    let ty = &module.types[handle];
    match &ty.name {
        Some(name) => name.clone(),
        None => render(module, &ty.inner, language),
    }
}

/// Render a resolved type.
pub fn render_resolution(
    module: &Module,
    resolution: &TypeResolution,
    language: Language,
) -> String {
    match resolution {
        TypeResolution::Handle(handle) => render_handle(module, *handle, language),
        TypeResolution::Value(inner) => render(module, inner, language),
    }
}

/// Render a type's structure.
pub fn render(module: &Module, inner: &TypeInner, language: Language) -> String {
    match inner {
        TypeInner::Scalar(scalar) => scalar_name(*scalar, language).to_string(),
        TypeInner::Vector { size, scalar } => vector_name(*size, *scalar, language),
        TypeInner::Matrix { columns, rows, scalar } => {
            matrix_name(*columns, *rows, *scalar, language)
        }
        TypeInner::Atomic(scalar) => match language {
            Language::Wgsl => format!("atomic<{}>", scalar_name(*scalar, language)),
            Language::Glsl => "atomic_uint".to_string(),
        },
        TypeInner::Pointer { base, space } => match language {
            Language::Wgsl => format!(
                "ptr<{}, {}>",
                address_space(*space),
                render_handle(module, *base, language)
            ),
            Language::Glsl => render_handle(module, *base, language),
        },
        TypeInner::ValuePointer { size, scalar, space } => {
            let pointee = match size {
                Some(size) => vector_name(*size, *scalar, language),
                None => scalar_name(*scalar, language).to_string(),
            };
            match language {
                Language::Wgsl => format!("ptr<{}, {pointee}>", address_space(*space)),
                Language::Glsl => pointee,
            }
        }
        TypeInner::Array { base, size, .. } => {
            let base = render_handle(module, *base, language);
            match (language, size) {
                (Language::Wgsl, ArraySize::Constant(n)) => format!("array<{base}, {n}>"),
                (Language::Wgsl, _) => format!("array<{base}>"),
                (Language::Glsl, ArraySize::Constant(n)) => format!("{base}[{n}]"),
                (Language::Glsl, _) => format!("{base}[]"),
            }
        }
        TypeInner::BindingArray { base, size } => {
            let base = render_handle(module, *base, language);
            match size {
                ArraySize::Constant(n) => format!("binding_array<{base}, {n}>"),
                _ => format!("binding_array<{base}>"),
            }
        }
        TypeInner::Struct { .. } => "struct".to_string(),
        TypeInner::Image { dim, arrayed, class } => image_name(*dim, *arrayed, *class, language),
        TypeInner::Sampler { comparison } => match (language, comparison) {
            (Language::Wgsl, true) => "sampler_comparison".to_string(),
            (Language::Wgsl, false) => "sampler".to_string(),
            (Language::Glsl, true) => "samplerShadow".to_string(),
            (Language::Glsl, false) => "sampler".to_string(),
        },
        TypeInner::AccelerationStructure { .. } => "acceleration_structure".to_string(),
        TypeInner::RayQuery { .. } => "ray_query".to_string(),
        TypeInner::CooperativeMatrix { .. } => "cooperative_matrix".to_string(),
    }
}

fn scalar_name(scalar: Scalar, language: Language) -> &'static str {
    match (language, scalar.kind, scalar.width) {
        (Language::Wgsl, ScalarKind::Float, 2) => "f16",
        (Language::Wgsl, ScalarKind::Float, 8) => "f64",
        (Language::Wgsl, ScalarKind::Float, _) => "f32",
        (Language::Wgsl, ScalarKind::Sint, _) => "i32",
        (Language::Wgsl, ScalarKind::Uint, _) => "u32",
        (Language::Wgsl, ScalarKind::Bool, _) => "bool",
        // Abstract types are an internal stage of WGSL's type inference; they
        // are spelled as the concrete type they will settle into.
        (Language::Wgsl, ScalarKind::AbstractInt, _) => "i32",
        (Language::Wgsl, ScalarKind::AbstractFloat, _) => "f32",
        (Language::Glsl, ScalarKind::Float, 8) => "double",
        (Language::Glsl, ScalarKind::Float, _) => "float",
        (Language::Glsl, ScalarKind::Sint, _) => "int",
        (Language::Glsl, ScalarKind::Uint, _) => "uint",
        (Language::Glsl, ScalarKind::Bool, _) => "bool",
        (Language::Glsl, ScalarKind::AbstractInt, _) => "int",
        (Language::Glsl, ScalarKind::AbstractFloat, _) => "float",
    }
}

/// GLSL spells the component type as a prefix (`ivec3`); WGSL as an argument.
fn vector_name(size: VectorSize, scalar: Scalar, language: Language) -> String {
    let n = size as u8;
    match language {
        Language::Wgsl => format!("vec{n}<{}>", scalar_name(scalar, language)),
        Language::Glsl => {
            let prefix = match (scalar.kind, scalar.width) {
                (ScalarKind::Sint | ScalarKind::AbstractInt, _) => "i",
                (ScalarKind::Uint, _) => "u",
                (ScalarKind::Bool, _) => "b",
                (ScalarKind::Float | ScalarKind::AbstractFloat, 8) => "d",
                _ => "",
            };
            format!("{prefix}vec{n}")
        }
    }
}

fn matrix_name(columns: VectorSize, rows: VectorSize, scalar: Scalar, language: Language) -> String {
    let (c, r) = (columns as u8, rows as u8);
    match language {
        Language::Wgsl => format!("mat{c}x{r}<{}>", scalar_name(scalar, language)),
        Language::Glsl => {
            let prefix = if scalar.width == 8 { "d" } else { "" };
            if c == r {
                format!("{prefix}mat{c}")
            } else {
                format!("{prefix}mat{c}x{r}")
            }
        }
    }
}

fn address_space(space: AddressSpace) -> &'static str {
    match space {
        AddressSpace::Function => "function",
        AddressSpace::Private => "private",
        AddressSpace::WorkGroup => "workgroup",
        AddressSpace::Uniform => "uniform",
        AddressSpace::Storage { .. } => "storage",
        AddressSpace::Handle => "handle",
        AddressSpace::Immediate => "immediate",
        AddressSpace::TaskPayload => "task_payload",
        AddressSpace::RayPayload => "ray_payload",
        AddressSpace::IncomingRayPayload => "incoming_ray_payload",
    }
}

fn image_name(
    dim: ImageDimension,
    arrayed: bool,
    class: ImageClass,
    language: Language,
) -> String {
    let shape = match (dim, arrayed) {
        (ImageDimension::D1, false) => "1d",
        (ImageDimension::D1, true) => "1d_array",
        (ImageDimension::D2, false) => "2d",
        (ImageDimension::D2, true) => "2d_array",
        (ImageDimension::D3, _) => "3d",
        (ImageDimension::Cube, false) => "cube",
        (ImageDimension::Cube, true) => "cube_array",
    };
    match language {
        Language::Wgsl => match class {
            ImageClass::Sampled { kind, multi } => {
                let scalar = scalar_name(Scalar { kind, width: 4 }, language);
                if multi {
                    format!("texture_multisampled_{shape}<{scalar}>")
                } else {
                    format!("texture_{shape}<{scalar}>")
                }
            }
            ImageClass::Depth { multi } if multi => {
                format!("texture_depth_multisampled_{shape}")
            }
            ImageClass::Depth { .. } => format!("texture_depth_{shape}"),
            ImageClass::Storage { .. } => format!("texture_storage_{shape}"),
            ImageClass::External => "texture_external".to_string(),
        },
        Language::Glsl => {
            // GLSL's own spelling: `sampler2DArray`, `usampler3D`, `image2D`.
            let shape = match (dim, arrayed) {
                (ImageDimension::D1, false) => "1D",
                (ImageDimension::D1, true) => "1DArray",
                (ImageDimension::D2, false) => "2D",
                (ImageDimension::D2, true) => "2DArray",
                (ImageDimension::D3, _) => "3D",
                (ImageDimension::Cube, false) => "Cube",
                (ImageDimension::Cube, true) => "CubeArray",
            };
            match class {
                ImageClass::Sampled { kind, multi } => {
                    let prefix = match kind {
                        ScalarKind::Sint | ScalarKind::AbstractInt => "i",
                        ScalarKind::Uint => "u",
                        _ => "",
                    };
                    let multi = if multi { "MS" } else { "" };
                    format!("{prefix}sampler{shape}{multi}")
                }
                ImageClass::Depth { .. } => format!("sampler{shape}Shadow"),
                ImageClass::Storage { .. } => format!("image{shape}"),
                ImageClass::External => "samplerExternalOES".to_string(),
            }
        }
    }
}

/// The members reachable through `.` from a value of this type.
pub fn members(module: &Module, inner: &TypeInner, language: Language) -> Vec<Member> {
    match inner {
        TypeInner::Struct { members, .. } => members
            .iter()
            .filter_map(|member| {
                Some(Member {
                    name: member.name.clone()?,
                    type_name: render_handle(module, member.ty, language),
                    detail: "field",
                })
            })
            .collect(),
        TypeInner::Vector { size, scalar } => swizzles(*size, *scalar, language),
        // A pointer's members are its pointee's: WGSL 1.0 auto-dereferences
        // `p.x`, and GLSL has no pointers at all.
        TypeInner::Pointer { base, .. } => {
            members(module, &module.types[*base].inner, language)
        }
        TypeInner::ValuePointer { size: Some(size), scalar, .. } => {
            swizzles(*size, *scalar, language)
        }
        _ => Vec::new(),
    }
}

/// Vector component names.
///
/// Both spelling sets, and only the useful prefixes of each rather than the
/// full cross product — offering all 340 swizzles of a `vec4` is offering none.
fn swizzles(size: VectorSize, scalar: Scalar, language: Language) -> Vec<Member> {
    const POSITION: [&str; 4] = ["x", "y", "z", "w"];
    const COLOUR: [&str; 4] = ["r", "g", "b", "a"];
    let n = size as usize;

    let mut members = Vec::with_capacity(n * 2 + 4);
    for set in [POSITION, COLOUR] {
        for component in set.iter().take(n) {
            members.push(Member {
                name: component.to_string(),
                type_name: scalar_name(scalar, language).to_string(),
                detail: "component",
            });
        }
        // The prefixes: `xy`, `xyz`, `xyzw`, as far as the width allows.
        for width in 2..=n {
            let name: String = set[..width].concat();
            members.push(Member {
                name,
                type_name: vector_name(
                    match width {
                        2 => VectorSize::Bi,
                        3 => VectorSize::Tri,
                        _ => VectorSize::Quad,
                    },
                    scalar,
                    language,
                ),
                detail: "swizzle",
            });
        }
    }
    members
}

/// A function's expression types, resolved once and indexed by handle.
///
/// naga's own `Typifier` is private, but [`ResolveContext::resolve`] is not,
/// and expressions in an arena only ever refer to earlier ones — so one
/// forward pass resolves them all.
pub struct FunctionTypes {
    resolved: Vec<Option<TypeResolution>>,
}

impl FunctionTypes {
    pub fn of(module: &Module, function: &Function) -> FunctionTypes {
        let context = ResolveContext::with_locals(
            module,
            &function.local_variables,
            &function.arguments,
        );
        let mut resolved: Vec<Option<TypeResolution>> =
            Vec::with_capacity(function.expressions.len());

        for (_, expression) in function.expressions.iter() {
            // The borrow of `resolved` has to end before the push, hence the
            // block: the closure reads what earlier iterations wrote.
            let next = {
                let past = &resolved;
                context
                    .resolve(expression, |handle| {
                        past.get(handle.index()).and_then(Option::as_ref).ok_or_else(|| {
                            ResolveError::IncompatibleOperands(
                                "unresolved operand".to_string(),
                            )
                        })
                    })
                    .ok()
            };
            resolved.push(next);
        }

        FunctionTypes { resolved }
    }

    pub fn get(&self, handle: Handle<naga::Expression>) -> Option<&TypeResolution> {
        self.resolved.get(handle.index()).and_then(Option::as_ref)
    }
}

/// The function a name belongs to, entry points included.
pub fn function_named<'a>(module: &'a Module, name: &str) -> Option<&'a Function> {
    module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some(name))
        .map(|(_, f)| f)
        .or_else(|| {
            module
                .entry_points
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| &entry.function)
        })
}

/// The type of a bare name, looked up in a function's scope and then the
/// module's.
pub fn type_of_name(
    module: &Module,
    function: Option<&Function>,
    name: &str,
) -> Option<TypeResolution> {
    if let Some(function) = function {
        if let Some((_, local)) =
            function.local_variables.iter().find(|(_, l)| l.name.as_deref() == Some(name))
        {
            return Some(TypeResolution::Handle(local.ty));
        }
        if let Some(argument) =
            function.arguments.iter().find(|a| a.name.as_deref() == Some(name))
        {
            return Some(TypeResolution::Handle(argument.ty));
        }
        // A WGSL `let` is a named expression, not a variable, so its type has
        // to be inferred rather than read off a declaration.
        if let Some((handle, _)) =
            function.named_expressions.iter().find(|(_, n)| n.as_str() == name)
        {
            return FunctionTypes::of(module, function).get(*handle).cloned();
        }
    }

    if let Some((_, global)) =
        module.global_variables.iter().find(|(_, g)| g.name.as_deref() == Some(name))
    {
        return Some(TypeResolution::Handle(global.ty));
    }
    if let Some((_, constant)) =
        module.constants.iter().find(|(_, c)| c.name.as_deref() == Some(name))
    {
        return Some(TypeResolution::Handle(constant.ty));
    }
    if let Some((_, over)) =
        module.overrides.iter().find(|(_, o)| o.name.as_deref() == Some(name))
    {
        return Some(TypeResolution::Handle(over.ty));
    }
    // A bare type name — `Camera` in `var c: Camera` — types itself.
    module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some(name))
        .map(|(handle, _)| TypeResolution::Handle(handle))
}

/// Walk `base.field.field…`, returning the type the chain arrives at.
pub fn type_of_chain(
    module: &Module,
    function: Option<&Function>,
    base: &str,
    fields: &[&str],
) -> Option<TypeResolution> {
    let mut current = type_of_name(module, function, base)?;
    for field in fields {
        let inner = current.inner_with(&module.types);
        current = match inner {
            TypeInner::Struct { members, .. } => {
                let member =
                    members.iter().find(|m| m.name.as_deref() == Some(*field))?;
                TypeResolution::Handle(member.ty)
            }
            TypeInner::Pointer { base, .. } => {
                let members = match &module.types[*base].inner {
                    TypeInner::Struct { members, .. } => members,
                    _ => return None,
                };
                let member =
                    members.iter().find(|m| m.name.as_deref() == Some(*field))?;
                TypeResolution::Handle(member.ty)
            }
            // A swizzle narrows or widens; its component type is unchanged.
            TypeInner::Vector { scalar, .. } => {
                let width = field.len();
                let scalar = *scalar;
                match width {
                    1 => TypeResolution::Value(TypeInner::Scalar(scalar)),
                    2 => TypeResolution::Value(TypeInner::Vector {
                        size: VectorSize::Bi,
                        scalar,
                    }),
                    3 => TypeResolution::Value(TypeInner::Vector {
                        size: VectorSize::Tri,
                        scalar,
                    }),
                    4 => TypeResolution::Value(TypeInner::Vector {
                        size: VectorSize::Quad,
                        scalar,
                    }),
                    _ => return None,
                }
            }
            _ => return None,
        };
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Analysis;
    use crate::analysis::dialect::Dialect;

    fn module(source: &str, language: Language, extension: &str) -> std::rc::Rc<Module> {
        // These fixtures exist to be parsed, so ask for the dialect naga
        // implements rather than leaving it to a look at the source.
        let analysis = Analysis::run(source, language, extension, Dialect::Vulkan);
        assert!(analysis.problems.is_empty(), "{:?}", analysis.problems);
        analysis.module.expect("naga parsed it")
    }

    const WGSL: &str = "
struct Camera { view: mat4x4<f32>, eye: vec3<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var albedo: texture_2d<f32>;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let scaled = uv * 2.0;
    return vec4<f32>(scaled, camera.eye.x, 1.0);
}
";

    #[test]
    fn types_render_in_the_spelling_of_their_language() {
        let module = module(WGSL, Language::Wgsl, "wgsl");
        let camera = type_of_name(&module, None, "camera").unwrap();
        assert_eq!(render_resolution(&module, &camera, Language::Wgsl), "Camera");

        let view = type_of_chain(&module, None, "camera", &["view"]).unwrap();
        assert_eq!(render_resolution(&module, &view, Language::Wgsl), "mat4x4<f32>");
        // The same naga type, spelled the GLSL way.
        assert_eq!(render_resolution(&module, &view, Language::Glsl), "mat4");

        let albedo = type_of_name(&module, None, "albedo").unwrap();
        assert_eq!(render_resolution(&module, &albedo, Language::Wgsl), "texture_2d<f32>");
        assert_eq!(render_resolution(&module, &albedo, Language::Glsl), "sampler2D");
    }

    #[test]
    fn a_chain_walks_through_a_struct_and_then_a_swizzle() {
        let module = module(WGSL, Language::Wgsl, "wgsl");
        let x = type_of_chain(&module, None, "camera", &["eye", "x"]).unwrap();
        assert_eq!(render_resolution(&module, &x, Language::Wgsl), "f32");
        let xy = type_of_chain(&module, None, "camera", &["eye", "xy"]).unwrap();
        assert_eq!(render_resolution(&module, &xy, Language::Wgsl), "vec2<f32>");
        // A field that does not exist resolves to nothing rather than guessing.
        assert!(type_of_chain(&module, None, "camera", &["nope"]).is_none());
    }

    /// A WGSL `let` has no declared type; the only way to know it is to infer
    /// it, which is what `ResolveContext` is here for.
    #[test]
    fn a_let_binding_gets_its_inferred_type() {
        let module = module(WGSL, Language::Wgsl, "wgsl");
        let function = function_named(&module, "main").expect("entry point");
        let scaled = type_of_name(&module, Some(function), "scaled").unwrap();
        assert_eq!(render_resolution(&module, &scaled, Language::Wgsl), "vec2<f32>");
    }

    #[test]
    fn a_parameter_resolves_within_its_function_only() {
        let module = module(WGSL, Language::Wgsl, "wgsl");
        let function = function_named(&module, "main").expect("entry point");
        let uv = type_of_name(&module, Some(function), "uv").unwrap();
        assert_eq!(render_resolution(&module, &uv, Language::Wgsl), "vec2<f32>");
        assert!(type_of_name(&module, None, "uv").is_none());
    }

    #[test]
    fn struct_members_and_swizzles_are_both_offered() {
        let module = module(WGSL, Language::Wgsl, "wgsl");
        let camera = type_of_name(&module, None, "camera").unwrap();
        let fields = members(&module, camera.inner_with(&module.types), Language::Wgsl);
        let names: Vec<&str> = fields.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["view", "eye"]);

        let eye = type_of_chain(&module, None, "camera", &["eye"]).unwrap();
        let names: Vec<String> = members(&module, eye.inner_with(&module.types), Language::Wgsl)
            .iter()
            .map(|m| m.name.clone())
            .collect();
        // Both spelling sets, single components and prefixes, nothing beyond.
        assert_eq!(names, ["x", "y", "z", "xy", "xyz", "r", "g", "b", "rg", "rgb"]);
    }

    #[test]
    fn glsl_types_resolve_the_same_way() {
        const GLSL: &str = "#version 450
struct Light { vec3 colour; float intensity; };
layout(binding = 0) uniform Light light;
layout(location = 0) out vec4 fragColour;
void main() { fragColour = vec4(light.colour, 1.0); }
";
        let module = module(GLSL, Language::Glsl, "frag");
        let colour =
            type_of_chain(&module, None, "light", &["colour"]).unwrap();
        assert_eq!(render_resolution(&module, &colour, Language::Glsl), "vec3");
        let out = type_of_name(&module, None, "fragColour").unwrap();
        assert_eq!(render_resolution(&module, &out, Language::Glsl), "vec4");
    }
}
