//! D228: traits and conformances.
//!
//! `trait Resource { fn drop() }` declares the members a conforming type
//! supplies; `conform File to Resource { fn drop() { … } }` is the only way to
//! conform. This pass resolves every trait a conformance names, checks the
//! conformance against them (where it is written, the members it supplies and
//! their signatures, the traits' parents), moves its methods into the type
//! (public, as a trait's members are), records the traits on the type
//! (`TypeDef::traits`), and removes the `conform` items; later passes skip the
//! traits. A name means nothing by itself: a type's own `fn drop()` is an
//! ordinary method.
//!
//! A member with a body is provided: each conforming type that does not
//! implement it gets a copy that resolves in the trait's module and sees only
//! the trait's members (`Method::provided`), and the trait's archetype
//! (`Self_Name`) checks the body once against the trait alone.
//!
//! Not yet (later slices of the freeze): getters and setters, generic and
//! built-in types as targets, members written outside the type's module.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::*;
use crate::error::{Error, Result};
use crate::loader::{self, BoundImport, Package};
use crate::span::Span;

/// std's core module, in every package: its exported traits are visible by
/// name everywhere.
const CORE_MODULE: &str = "std_core";

/// Built-in type names: a conformance cannot name one yet.
const BUILT_IN_TYPES: &[&str] = &[
    "Int64", "Int32", "UInt64", "UInt32", "UInt8", "Byte", "Float64", "Bool", "String",
    "Bytes", "Buffer", "Void", "Array", "Map", "Node", "Element", "Text", "Fragment", "Attr",
    "Address",
];

/// A type's own methods (accessors among them) and fields, by name.
#[derive(Default)]
struct OwnMembers {
    methods: HashMap<String, Method>,
    fields: HashMap<String, TypeExpr>,
}

struct TraitEntry {
    def: TraitDef,
    path: PathBuf,
    parents: Vec<TraitRef>,
}

/// One `conform` item, taken out of its module.
struct Pending {
    module: String,
    path: PathBuf,
    conformance: Conformance,
}

/// Names visible in each module: declared types and traits, and imports.
struct Scope<'a> {
    imports: &'a HashMap<String, HashMap<String, BoundImport>>,
    types: HashSet<(String, String)>,
    traits: &'a HashMap<TraitRef, TraitEntry>,
    /// std/core's exported types, visible by name everywhere (`Order`).
    core_types: HashSet<String>,
}

impl Scope<'_> {
    fn import(&self, module: &str, name: &str) -> Option<(String, String)> {
        match self.imports.get(module)?.get(name)? {
            BoundImport::Name {
                from_module,
                original,
            } => Some((from_module.clone(), original.clone())),
            BoundImport::Namespace { .. } => None,
        }
    }

    fn trait_named(&self, module: &str, name: &str) -> Option<TraitRef> {
        let local = TraitRef {
            module: module.to_string(),
            name: name.to_string(),
        };
        if self.traits.contains_key(&local) {
            return Some(local);
        }
        if let Some((from_module, original)) = self.import(module, name) {
            let imported = TraitRef {
                module: from_module,
                name: original,
            };
            return self.traits.contains_key(&imported).then_some(imported);
        }
        let core = TraitRef {
            module: CORE_MODULE.to_string(),
            name: name.to_string(),
        };
        self.traits
            .get(&core)
            .is_some_and(|entry| entry.def.exported)
            .then_some(core)
    }

    fn type_named(&self, module: &str, name: &str) -> Option<(String, String)> {
        let local = (module.to_string(), name.to_string());
        if self.types.contains(&local) {
            return Some(local);
        }
        if let Some(imported) = self.import(module, name).filter(|imported| self.types.contains(imported)) {
            return Some(imported);
        }
        self.core_types
            .contains(name)
            .then(|| (CORE_MODULE.to_string(), name.to_string()))
    }
}

/// Conformances of built-in types, by the type's name (`Int64`, `Array`),
/// each holding for the instances whose arguments meet its bounds.
pub type BuiltInConformances = HashMap<String, Vec<ConditionalConformance>>;

pub fn desugar_package(package: &mut Package) -> Result<BuiltInConformances> {
    let has_traits = package.modules.iter().any(|module| {
        module
            .ast
            .items
            .iter()
            .any(|item| matches!(item, Item::Trait(_) | Item::Conform(_)))
    });
    if !has_traits {
        return Ok(BuiltInConformances::new());
    }
    let imports = loader::build_import_maps(package)?;
    let mut errors: Vec<Error> = Vec::new();

    // Declared types, by module, for resolving names.
    let mut types: HashSet<(String, String)> = HashSet::new();
    for module in &package.modules {
        for item in &module.ast.items {
            if let Item::Type(def) = item {
                types.insert((module.id.clone(), def.name.clone()));
            }
        }
    }

    // Traits, each checked on its own; `keys` keeps source order for errors.
    let mut traits: HashMap<TraitRef, TraitEntry> = HashMap::new();
    let mut keys: Vec<TraitRef> = Vec::new();
    for module in &package.modules {
        for item in &module.ast.items {
            let Item::Trait(def) = item else { continue };
            let key = TraitRef {
                module: module.id.clone(),
                name: def.name.clone(),
            };
            let error = if traits.contains_key(&key) {
                Some(Error::at(format!("duplicate trait `{}`", def.name), def.name_span))
            } else if types.contains(&(module.id.clone(), def.name.clone())) {
                Some(Error::at(
                    format!("`{}` is declared as both a type and a trait", def.name),
                    def.name_span,
                ))
            } else {
                check_trait(def).err()
            };
            if let Some(error) = error {
                errors.push(error.with_path(module.path.clone()));
                continue;
            }
            keys.push(key.clone());
            traits.insert(
                key,
                TraitEntry {
                    def: def.clone(),
                    path: module.path.clone(),
                    parents: Vec::new(),
                },
            );
        }
    }

    // Parents, resolved in the trait's module.
    for key in &keys {
        let entry = &traits[key];
        let mut parents = Vec::new();
        for (name, span) in &entry.def.parents {
            let scope = Scope {
                imports: &imports,
                types: HashSet::new(),
                traits: &traits,
                core_types: HashSet::new(),
            };
            match scope.trait_named(&key.module, name) {
                Some(parent) if parent == *key => errors.push(
                    Error::at(format!("`{name}` cannot be its own parent"), *span)
                        .with_path(entry.path.clone()),
                ),
                Some(parent) => parents.push(parent),
                None => errors.push(
                    Error::at(format!("unknown trait `{name}`"), *span)
                        .with_path(entry.path.clone()),
                ),
            }
        }
        traits.get_mut(key).unwrap().parents = parents;
    }
    for key in &keys {
        if let Some(cycle) = parent_cycle(key, &traits) {
            let entry = &traits[key];
            errors.push(
                Error::at(
                    format!("trait `{}` is its own ancestor ({cycle})", key.name),
                    entry.def.name_span,
                )
                .with_path(entry.path.clone()),
            );
        }
    }
    if !errors.is_empty() {
        return Err(Error::multi(errors));
    }

    // Each trait with provided bodies gets its archetype, in its module.
    for key in &keys {
        if let Some(def) = archetype(key, &traits) {
            if let Some(module) = package.modules.iter_mut().find(|module| module.id == key.module) {
                module.ast.items.push(Item::Type(def));
            }
        }
    }

    // Take every conformance out of its module.
    let mut pending: Vec<Pending> = Vec::new();
    for module in &mut package.modules {
        for item in &module.ast.items {
            if let Item::Conform(conformance) = item {
                pending.push(Pending {
                    module: module.id.clone(),
                    path: module.path.clone(),
                    conformance: conformance.clone(),
                });
            }
        }
        // Traits stay: an import of one is checked against the exports again.
        module
            .ast
            .items
            .retain(|item| !matches!(item, Item::Conform(_)));
    }

    let core_types = package
        .modules
        .iter()
        .filter(|module| module.id == CORE_MODULE)
        .flat_map(|module| module.ast.items.iter())
        .filter_map(|item| match item {
            Item::Type(def) if def.exported => Some(def.name.clone()),
            _ => None,
        })
        .collect();
    let scope = Scope {
        imports: &imports,
        types,
        traits: &traits,
        core_types,
    };
    generic_functions(package, &scope)?;
    // The type's own members, before any conformance adds to them.
    let mut own_members: HashMap<(String, String), OwnMembers> = HashMap::new();
    let mut generic: HashMap<(String, String), usize> = HashMap::new();
    let mut unions: HashSet<(String, String)> = HashSet::new();
    for module in &package.modules {
        for item in &module.ast.items {
            let Item::Type(def) = item else { continue };
            let key = (module.id.clone(), def.name.clone());
            let members = OwnMembers {
                methods: def
                    .methods
                    .iter()
                    .map(|method| (method.name.clone(), method.clone()))
                    .collect(),
                fields: def
                    .params
                    .iter()
                    .map(|param| (param.name.clone(), param.ty.clone()))
                    .chain(def.fields.iter().map(|field| (field.name.clone(), field.ty.clone())))
                    .collect(),
            };
            own_members.insert(key.clone(), members);
            if def.union_members.is_some() {
                unions.insert(key.clone());
            }
            if !def.type_params.is_empty() {
                generic.insert(key, def.type_params.len());
            }
        }
    }

    // (type) → trait → where it was declared, and its bounds when the
    // conformance holds per instance; (type) → member → its trait.
    let mut conformed: Conformed = HashMap::new();
    let mut trait_members: HashMap<(String, String), HashMap<String, TraitRef>> = HashMap::new();
    let mut additions: HashMap<(String, String), Vec<Method>> = HashMap::new();
    let mut declared_at: Vec<((String, String), TraitRef, Span, PathBuf)> = Vec::new();
    let mut built_in: BuiltInConformances = HashMap::new();
    let mut derived: HashMap<(String, String), Vec<String>> = HashMap::new();

    for item in &pending {
        let result = check_conformance(
            item,
            &scope,
            &own_members,
            &generic,
            &unions,
            &mut conformed,
            &mut trait_members,
        );
        match result {
            Ok((target, refs, methods, defaults)) => {
                derived.entry(target.clone()).or_default().extend(defaults);
                for trait_ref in refs {
                    declared_at.push((
                        target.clone(),
                        trait_ref,
                        item.conformance.span,
                        item.path.clone(),
                    ));
                }
                additions.entry(target).or_default().extend(methods);
            }
            Err(error) => errors.push(error.with_path(item.path.clone())),
        }
    }

    // A trait's parents are conformed too, for every instance this one holds
    // for: a parent's bounds follow from this conformance's.
    for (target, trait_ref, span, path) in &declared_at {
        let bounds = &conformed[target][trait_ref].1;
        for parent in &traits[trait_ref].parents {
            let covered = conformed[target].get(parent).is_some_and(|(_, parent_bounds)| {
                match (parent_bounds, bounds) {
                    (None, _) => true,
                    (Some(parent_bounds), Some(bounds)) => {
                        parent_bounds.iter().zip(bounds).all(|(needed, given)| {
                            let given: Vec<TraitRef> =
                                given.iter().flat_map(|each| lineage(each, &traits)).collect();
                            needed.iter().all(|each| given.contains(each))
                        })
                    }
                    (Some(_), None) => false,
                }
            });
            if !covered {
                errors.push(
                    Error::at(
                        format!(
                            "`{}` conforms to `{}`, so it must conform to `{}` too",
                            target.1, trait_ref.name, parent.name
                        ),
                        *span,
                    )
                    .with_help(format!("add `conform {} to {}`", target.1, parent.name))
                    .with_path(path.clone()),
                );
            }
        }
    }
    if !errors.is_empty() {
        return Err(Error::multi(errors));
    }

    for (target, done) in &conformed {
        if target.0 != BUILT_IN_MODULE || scope.types.contains(target) {
            continue;
        }
        let mut list: Vec<ConditionalConformance> = done
            .iter()
            .map(|(trait_ref, (_, bounds))| ConditionalConformance {
                trait_ref: trait_ref.clone(),
                bounds: bounds.clone().unwrap_or_default(),
            })
            .collect();
        list.sort_by(|a, b| a.trait_ref.name.cmp(&b.trait_ref.name));
        built_in.insert(target.1.clone(), list);
    }
    for module in &mut package.modules {
        for item in &mut module.ast.items {
            let Item::Type(def) = item else { continue };
            let key = (module.id.clone(), def.name.clone());
            if let Some(methods) = additions.remove(&key) {
                def.methods.extend(methods);
            }
            if let Some(names) = derived.remove(&key) {
                def.derived = names;
            }
            if let Some(done) = conformed.get(&key) {
                let mut plain: Vec<TraitRef> = Vec::new();
                for (trait_ref, (_, bounds)) in done {
                    match bounds {
                        None => plain.push(trait_ref.clone()),
                        Some(bounds) => def.conditional.push(ConditionalConformance {
                            trait_ref: trait_ref.clone(),
                            bounds: bounds.clone(),
                        }),
                    }
                }
                plain.sort_by(|a, b| (&a.module, &a.name).cmp(&(&b.module, &b.name)));
                def.traits = plain;
                def.conditional
                    .sort_by(|a, b| (&a.trait_ref.module, &a.trait_ref.name).cmp(&(&b.trait_ref.module, &b.trait_ref.name)));
            }
        }
    }
    Ok(built_in)
}

/// A target type → each trait it conforms to: where, and the bounds per type
/// parameter when it holds per instance (`None` for a plain type).
type Conformed = HashMap<(String, String), HashMap<TraitRef, (Span, Option<Vec<Vec<TraitRef>>>)>>;

/// Built-in types belong to std's core module, which declares their
/// conformances (D228).
const BUILT_IN_MODULE: &str = CORE_MODULE;

/// Built-in types a conformance may name, with their number of type
/// parameters.
const CONFORMABLE_BUILT_INS: &[(&str, usize)] = &[
    ("Int64", 0), ("Int32", 0), ("UInt64", 0), ("UInt32", 0), ("Byte", 0), ("Float64", 0),
    ("Bool", 0), ("String", 0), ("Bytes", 0), ("Buffer", 0), ("Void", 0), ("Node", 0),
    ("Array", 1), ("Map", 2),
];

/// Traits whose required members the compiler implements for a conformance
/// without them, field by field (D228 §4).
fn has_defaults(trait_ref: &TraitRef) -> bool {
    trait_ref.module == CORE_MODULE
        && matches!(trait_ref.name.as_str(), "Equatable" | "Hashable" | "Ordered")
}

/// A trait's members have distinct names and no parameter defaults.
fn check_trait(def: &TraitDef) -> Result<()> {
    let mut seen = HashSet::new();
    for member in &def.members {
        if !seen.insert(member.name.as_str()) {
            return Err(Error::at(
                format!("`{}` is declared twice in `{}`", member.name, def.name),
                member.name_span,
            ));
        }
        if let Some(param) = member.params.iter().find(|param| param.default.is_some()) {
            return Err(Error::at(
                "a trait member's parameters take no defaults",
                param.name_span,
            ));
        }
    }
    Ok(())
}

/// The chain that leads a trait back to itself, if one does.
fn parent_cycle(start: &TraitRef, traits: &HashMap<TraitRef, TraitEntry>) -> Option<String> {
    let mut path = vec![start.name.clone()];
    let mut seen = HashSet::new();
    fn walk(
        current: &TraitRef,
        start: &TraitRef,
        traits: &HashMap<TraitRef, TraitEntry>,
        seen: &mut HashSet<TraitRef>,
        path: &mut Vec<String>,
    ) -> bool {
        for parent in &traits[current].parents {
            path.push(parent.name.clone());
            if parent == start {
                return true;
            }
            if seen.insert(parent.clone()) && walk(parent, start, traits, seen, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    walk(start, start, traits, &mut seen, &mut path).then(|| path.join(" → "))
}

/// A checked conformance: its target, its traits, the methods it adds, and
/// which of them are the compiler's defaults.
type Checked = ((String, String), Vec<TraitRef>, Vec<Method>, Vec<String>);

fn check_conformance(
    item: &Pending,
    scope: &Scope,
    own_members: &HashMap<(String, String), OwnMembers>,
    generic: &HashMap<(String, String), usize>,
    unions: &HashSet<(String, String)>,
    conformed: &mut Conformed,
    trait_members: &mut HashMap<(String, String), HashMap<String, TraitRef>>,
) -> Result<Checked> {
    let conformance = &item.conformance;
    let ty = &conformance.ty;
    let built_in = CONFORMABLE_BUILT_INS
        .iter()
        .find(|(name, _)| *name == ty.name)
        .map(|(_, arity)| *arity);
    let (target, arity) = if let Some(arity) = built_in {
        ((BUILT_IN_MODULE.to_string(), ty.name.clone()), Some(arity))
    } else if BUILT_IN_TYPES.contains(&ty.name.as_str()) {
        return Err(Error::at(
            format!("the built-in type `{}` cannot conform to a trait", ty.name),
            ty.span,
        ));
    } else {
        let Some(target) = scope.type_named(&item.module, &ty.name) else {
            let message = if scope.trait_named(&item.module, &ty.name).is_some() {
                format!("`{}` is a trait; `conform` names a type first: `conform Type to {}`", ty.name, ty.name)
            } else {
                format!("unknown type `{}`", ty.name)
            };
            return Err(Error::at(message, ty.span));
        };
        let arity = generic.get(&target).copied();
        (target, arity)
    };
    // A generic or built-in target holds per instance, with bounds.
    let bounds: Option<Vec<Vec<TraitRef>>> = match arity {
        None if conformance.params.is_empty() => None,
        None => {
            return Err(Error::at(
                format!("`{}` takes no type parameters", ty.name),
                ty.span,
            ))
        }
        Some(_) if built_in == Some(0) && conformance.params.is_empty() => Some(Vec::new()),
        Some(count) if conformance.params.len() != count => {
            return Err(Error::at(
                format!(
                    "`{}` takes {count} type parameter{}: `conform {}<{}> to …`",
                    ty.name,
                    if count == 1 { "" } else { "s" },
                    ty.name,
                    ["T", "U", "V", "W"][..count.min(4)].join(", ")
                ),
                ty.span,
            ))
        }
        Some(_) => {
            let mut seen = HashSet::new();
            let mut lists = Vec::new();
            for param in &conformance.params {
                if !seen.insert(param.name.as_str()) {
                    return Err(Error::at(
                        format!("type parameter `{}` is named twice", param.name),
                        param.span,
                    ));
                }
                let mut list = Vec::new();
                for (name, span) in &param.traits {
                    let Some(bound) = scope.trait_named(&item.module, name) else {
                        return Err(Error::at(format!("unknown trait `{name}`"), *span));
                    };
                    list.push(bound);
                }
                lists.push(list);
            }
            Some(lists)
        }
    };
    if bounds.is_some() && !conformance.methods.is_empty() {
        return Err(Error::at(
            format!(
                "a conformance of `{}` with members is not supported yet (generic and built-in types conform without a body for now)",
                ty.name
            ),
            conformance.span,
        ));
    }

    let mut refs: Vec<(TraitRef, Span)> = Vec::new();
    for (name, span) in &conformance.traits {
        let Some(trait_ref) = scope.trait_named(&item.module, name) else {
            let message = if scope.type_named(&item.module, name).is_some() {
                format!("`{name}` is a type, not a trait")
            } else {
                format!("unknown trait `{name}`")
            };
            return Err(Error::at(message, *span));
        };
        if refs.iter().any(|(seen, _)| *seen == trait_ref) {
            return Err(Error::at(format!("`{name}` is listed twice"), *span));
        }
        if item.module != target.0 && item.module != trait_ref.module {
            return Err(Error::at(
                format!(
                    "a conformance goes in the module that defines `{}` or the one that defines `{name}`",
                    target.1
                ),
                conformance.span,
            ));
        }
        if conformed
            .get(&target)
            .is_some_and(|done| done.contains_key(&trait_ref))
        {
            return Err(Error::at(
                format!("`{}` already conforms to `{name}`", target.1),
                *span,
            ));
        }
        refs.push((trait_ref, *span));
    }
    if !conformance.methods.is_empty() && unions.contains(&target) {
        return Err(Error::at(
            format!(
                "`{}` is a union, and a union's methods are not supported yet",
                target.1
            ),
            conformance.span,
        ));
    }
    if !conformance.methods.is_empty() && item.module != target.0 {
        return Err(Error::at(
            format!(
                "a conformance with members goes in the module that defines `{}` (members in the trait's module are not supported yet)",
                target.1
            ),
            conformance.span,
        ));
    }

    // Each member goes to the listed trait that declares it.
    let mut implemented: HashMap<&str, &Method> = HashMap::new();
    for method in &conformance.methods {
        if implemented.insert(method.name.as_str(), method).is_some() {
            return Err(Error::at(
                format!("`{}` is implemented twice", method.name),
                method.name_span,
            ));
        }
        let declaring: Vec<&TraitRef> = refs
            .iter()
            .map(|(trait_ref, _)| trait_ref)
            .filter(|trait_ref| {
                scope.traits[*trait_ref]
                    .def
                    .members
                    .iter()
                    .any(|member| member.name == method.name)
            })
            .collect();
        match declaring.as_slice() {
            [] => {
                let listed: Vec<String> = refs
                    .iter()
                    .map(|(trait_ref, _)| format!("`{}`", trait_ref.name))
                    .collect();
                return Err(Error::at(
                    format!("`{}` is not a member of {}", method.name, listed.join(" or ")),
                    method.name_span,
                )
                .with_help(format!("a method of `{}` alone goes in its type's body", target.1)));
            }
            [_] => {}
            [first, second, ..] => {
                return Err(Error::at(
                    format!(
                        "`{}` is a member of both `{}` and `{}`",
                        method.name, first.name, second.name
                    ),
                    method.name_span,
                ))
            }
        }
    }

    // Every member supplied, with the trait's signature; no name taken twice.
    let empty = OwnMembers::default();
    let own = own_members.get(&target).unwrap_or(&empty);
    let mut methods = Vec::new();
    let mut derived = Vec::new();
    for (trait_ref, span) in &refs {
        let entry = &scope.traits[trait_ref];
        for member in &entry.def.members {
            let shown = member_display(&member.name);
            let clash = || {
                Error::at(
                    format!(
                        "`{}` already has a member named `{}`, so it cannot take `{}`'s",
                        target.1, shown, trait_ref.name
                    ),
                    *span,
                )
            };
            let taken = trait_members.entry(target.clone()).or_default();
            if let Some(other) = taken.get(&member.name) {
                return Err(Error::at(
                    format!(
                        "`{}` is a member of both `{}` and `{}`, and `{}` conforms to both",
                        shown, other.name, trait_ref.name, target.1
                    ),
                    *span,
                ));
            }
            taken.insert(member.name.clone(), trait_ref.clone());
            // A property's field (`area` for `get area`).
            let property = accessor_of(&member.name);
            let stored = property.and_then(|(_, name)| own.fields.get(name).map(|ty| (name, ty)));
            let own_method = own.methods.get(&member.name);
            if property.is_none() && own.fields.contains_key(&member.name) {
                return Err(clash());
            }
            match (implemented.get(member.name.as_str()), &member.body) {
                (Some(_), _) if own_method.is_some() => return Err(clash()),
                (Some(method), _) => {
                    methods.push(implementation(method, member, trait_ref, item, &target, scope)?)
                }
                // A required accessor the type's own accessor or field
                // satisfies: uniform access.
                (None, None) if property.is_some() && own_method.is_some() => {
                    let own_item = Pending {
                        module: target.0.clone(),
                        path: item.path.clone(),
                        conformance: item.conformance.clone(),
                    };
                    implementation(own_method.unwrap(), member, trait_ref, &own_item, &target, scope)?;
                }
                (None, None) if stored.is_some() => {
                    let (name, field_ty) = stored.unwrap();
                    let declared = match property.unwrap().0 {
                        AccessorKind::Get => member.return_type.as_ref(),
                        AccessorKind::Set => member.params.first().map(|param| &param.ty),
                    };
                    let matches = declared.is_some_and(|declared| {
                        canonical(field_ty, &target.0, &target, scope)
                            == canonical(declared, &trait_ref.module, &target, scope)
                    });
                    if !matches {
                        return Err(Error::at(
                            format!(
                                "field `{name}` of `{}` does not have the type `{}`'s `{}` asks for",
                                target.1, trait_ref.name, shown
                            ),
                            conformance.span,
                        ));
                    }
                    methods.push(field_accessor(&member.name, field_ty, conformance.span));
                }
                (None, Some(_)) if own_method.is_some() || stored.is_some() => return Err(clash()),
                (None, Some(body)) => methods.push(provided_method(
                    member,
                    body.clone(),
                    trait_ref,
                    visible_members(trait_ref, scope.traits),
                )),
                (None, None) if own_method.is_some() => return Err(clash()),
                // The compiler's field-by-field default (D228 §4): a method
                // of a plain product; a union or an instance compares
                // structurally without one.
                (None, None) if has_defaults(trait_ref) => {
                    if bounds.is_none() && !unions.contains(&target) {
                        methods.push(default_member(member, conformance.span));
                        derived.push(member.name.clone());
                    }
                }
                (None, None) => {
                    return Err(Error::at(
                        format!(
                            "`{}` does not implement `{}` from `{}`",
                            target.1,
                            signature(member, &target.1),
                            trait_ref.name
                        ),
                        conformance.span,
                    ))
                }
            }
        }
    }
    let done = conformed.entry(target.clone()).or_default();
    for (trait_ref, span) in &refs {
        done.insert(trait_ref.clone(), (*span, bounds.clone()));
    }
    Ok((
        target,
        refs.into_iter().map(|(trait_ref, _)| trait_ref).collect(),
        methods,
        derived,
    ))
}

/// The conformance's method, checked against the trait member it implements:
/// public, with the trait's return type when it writes none. `Self` stays: in
/// a type's methods it names the type.
fn implementation(
    method: &Method,
    member: &TraitMember,
    trait_ref: &TraitRef,
    item: &Pending,
    target: &(String, String),
    scope: &Scope,
) -> Result<Method> {
    let in_trait = |ty: &TypeExpr| canonical(ty, &trait_ref.module, target, scope);
    let in_conformance = |ty: &TypeExpr| canonical(ty, &item.module, target, scope);
    let mismatch = || {
        Error::at(
            format!(
                "`{}` must match `{}` in `{}`",
                method.name,
                signature(member, &target.1),
                trait_ref.name
            ),
            method.name_span,
        )
    };
    if method.params.len() != member.params.len() {
        return Err(mismatch());
    }
    for (written, declared) in method.params.iter().zip(&member.params) {
        if written.name != declared.name
            || written.positional != declared.positional
            || written.mutable != declared.mutable
            || in_conformance(&written.ty) != in_trait(&declared.ty)
        {
            return Err(mismatch());
        }
        if written.default.is_some() {
            return Err(Error::at(
                "a trait member's parameters take no defaults",
                written.name_span,
            ));
        }
    }
    let declared_return = member
        .return_type
        .clone()
        .unwrap_or_else(|| void_type(member.name_span));
    match &method.return_type {
        Some(written) if in_conformance(written) != in_trait(&declared_return) => {
            return Err(mismatch())
        }
        Some(_) => {}
        // The trait's return type, spelled as the trait spells it: it must
        // name the same types here.
        None if in_conformance(&declared_return) != in_trait(&declared_return) => {
            return Err(Error::at(
                format!(
                    "write `{}`'s return type: `{}`'s spelling does not name the same type here",
                    method.name, trait_ref.name
                ),
                method.name_span,
            ))
        }
        None => {}
    }
    let mut method = method.clone();
    method.exported = true;
    method.return_type.get_or_insert(declared_return);
    Ok(method)
}

/// A member a trait provides, as a method of a conforming type (or of the
/// trait's own archetype): public, resolved in the trait's module, seeing only
/// the trait's members.
fn provided_method(
    member: &TraitMember,
    body: Block,
    trait_ref: &TraitRef,
    members: Vec<String>,
) -> Method {
    Method {
        exported: true,
        name: member.name.clone(),
        name_span: member.name_span,
        params: member.params.clone(),
        return_type: member.return_type.clone(),
        body,
        provided: Some(Provided {
            module: trait_ref.module.clone(),
            members,
        }),
    }
}

/// A trait and its ancestors, nearest first, each once.
fn lineage(trait_ref: &TraitRef, traits: &HashMap<TraitRef, TraitEntry>) -> Vec<TraitRef> {
    let mut out = vec![trait_ref.clone()];
    let mut index = 0;
    while index < out.len() {
        for parent in &traits[&out[index]].parents {
            if !out.contains(parent) {
                out.push(parent.clone());
            }
        }
        index += 1;
    }
    out
}

/// The member names a trait's bodies see: its own and its ancestors'.
fn visible_members(trait_ref: &TraitRef, traits: &HashMap<TraitRef, TraitEntry>) -> Vec<String> {
    lineage(trait_ref, traits)
        .iter()
        .flat_map(|each| traits[each].def.members.iter().map(|member| member.name.clone()))
        .collect()
}

/// The type a trait's provided bodies are checked against, once, where the
/// trait is declared (D228): no fields, the trait's provided members, and a
/// stub for every other member of its lineage. Its name has an `_`, which no
/// source name has, and shows as the trait's (`Self_Named` is `Named`).
fn archetype(trait_ref: &TraitRef, traits: &HashMap<TraitRef, TraitEntry>) -> Option<TypeDef> {
    let entry = &traits[trait_ref];
    if entry.def.members.iter().all(|member| member.body.is_none()) {
        return None;
    }
    let members = visible_members(trait_ref, traits);
    let lineage = lineage(trait_ref, traits);
    let mut methods = Vec::new();
    for each in &lineage {
        for member in &traits[each].def.members {
            let body = match (&member.body, each == trait_ref) {
                (Some(body), true) => body.clone(),
                _ => stub_body(member.name_span),
            };
            methods.push(provided_method(member, body, each, members.clone()));
        }
    }
    Some(TypeDef {
        exported: false,
        name: format!("Self_{}", trait_ref.name),
        name_span: entry.def.name_span,
        type_params: Vec::new(),
        params: Vec::new(),
        fields: Vec::new(),
        methods,
        routes: Vec::new(),
        conversions: Vec::new(),
        union_members: None,
        traits: lineage.into_iter().filter(|each| !each.is_resource()).collect(),
        conditional: Vec::new(),
        derived: Vec::new(),
    })
}

/// The accessor a stored field gives a trait's `get` / `set` requirement: it
/// reads or writes `field` (D228).
fn field_accessor(method: &str, field_ty: &TypeExpr, span: Span) -> Method {
    let name = |text: &str| Expr {
        kind: ExprKind::Name(text.to_string()),
        span,
    };
    let (params, return_type, body) = match accessor_of(method) {
        Some((AccessorKind::Set, _)) => (
            vec![Param {
                name: "value".into(),
                name_span: span,
                positional: true,
                mutable: false,
                ty: field_ty.clone(),
                default: None,
            }],
            None,
            Block {
                items: vec![BlockItem::Assign {
                    target: AssignTarget::Name {
                        name: "field".into(),
                        name_span: span,
                    },
                    value: name("value"),
                }],
                tail: None,
                span,
            },
        ),
        _ => (
            Vec::new(),
            Some(field_ty.clone()),
            Block {
                items: Vec::new(),
                tail: Some(Box::new(name("field"))),
                span,
            },
        ),
    };
    Method {
        exported: true,
        name: method.to_string(),
        name_span: span,
        params,
        return_type,
        body,
        provided: None,
    }
}

/// A member the compiler writes for a conformance that leaves it out (D228
/// §4): `equals` is `self == other`, `compare` the field-by-field order, and
/// `hash` feeds each field to the hasher, all lowered structurally because the
/// type marks them derived.
fn default_member(member: &TraitMember, span: Span) -> Method {
    let name = |text: &str| Expr {
        kind: ExprKind::Name(text.to_string()),
        span,
    };
    let value = match member.name.as_str() {
        "equals" => ExprKind::Binary {
            op: BinaryOp::Eq,
            left: Box::new(name("self")),
            right: Box::new(name("other")),
        },
        "hash" => ExprKind::Call {
            callee: "__hash".into(),
            args: vec![
                Arg::Pos(name("self")),
                Arg::Pos(Expr {
                    kind: ExprKind::MutArg("hasher".into()),
                    span,
                }),
            ],
        },
        _ => ExprKind::Call {
            callee: "__compare".into(),
            args: vec![Arg::Pos(name("self")), Arg::Pos(name("other"))],
        },
    };
    Method {
        exported: true,
        name: member.name.clone(),
        name_span: span,
        params: member.params.clone(),
        return_type: member.return_type.clone(),
        body: Block {
            items: Vec::new(),
            tail: Some(Box::new(Expr { kind: value, span })),
            span,
        },
        provided: None,
    }
}

/// Generic functions (D228 §5): a bare trait as a parameter's type becomes a
/// hidden type parameter bounded by it (`report(shape: Shape)` is
/// `report<S: Shape>(shape: S)`); each type parameter's bounds are resolved,
/// and gets an archetype in the function's module (`Param_largest_T`, shown
/// as `T`) with a stub for every member of its bounds, so the body is checked
/// once against the bounds alone.
fn generic_functions(package: &mut Package, scope: &Scope) -> Result<()> {
    let mut errors = Vec::new();
    for module in &mut package.modules {
        let mut archetypes = Vec::new();
        for item in &mut module.ast.items {
            let Item::Function(function) = item else { continue };
            if let Err(error) = resolve_generics(function, &module.id, scope, &mut archetypes) {
                errors.push(error.with_path(module.path.clone()));
            }
        }
        module.ast.items.extend(archetypes.into_iter().map(Item::Type));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::multi(errors))
    }
}

fn resolve_generics(
    function: &mut Function,
    module: &str,
    scope: &Scope,
    archetypes: &mut Vec<TypeDef>,
) -> Result<()> {
    // A bare trait in a parameter is a hidden type parameter, one per parameter.
    for param in &mut function.params {
        let ty = &param.ty;
        if ty.args.is_empty()
            && ty.lit.is_none()
            && ty.union_members.is_none()
            && ty.fn_type.is_none()
            && scope.type_named(module, &ty.name).is_none()
            && !function.type_params.iter().any(|each| each.name == ty.name)
            && scope.trait_named(module, &ty.name).is_some()
        {
            let hidden = format!("{}_{}", ty.name, param.name);
            function.type_params.push(TypeParamBound {
                name: hidden.clone(),
                span: ty.span,
                traits: vec![(ty.name.clone(), ty.span)],
            });
            param.ty.name = hidden;
        }
    }
    if let Some(ret) = &function.return_type {
        if ret.args.is_empty() && scope.type_named(module, &ret.name).is_none() && scope.trait_named(module, &ret.name).is_some() {
            return Err(Error::at(
                format!("`{}` is a trait: a return type is a type or a type parameter", ret.name),
                ret.span,
            )
            .with_help(format!("return a concrete type, or add a type parameter: `fn {}<T: {}>(…) -> T`", function.name, ret.name)));
        }
    }
    if function.type_params.is_empty() {
        return Ok(());
    }
    let mut seen = HashSet::new();
    for param in &function.type_params {
        if !seen.insert(param.name.as_str()) {
            return Err(Error::at(format!("type parameter `{}` is named twice", param.name), param.span));
        }
        if scope.type_named(module, &param.name).is_some() {
            return Err(Error::at(
                format!("type parameter `{}` has the name of a type", param.name),
                param.span,
            ));
        }
        let mut bounds = Vec::new();
        for (name, span) in &param.traits {
            let Some(bound) = scope.trait_named(module, name) else {
                return Err(Error::at(format!("unknown trait `{name}`"), *span));
            };
            bounds.push(bound);
        }
        // No `__` in a type's name: the C name of its methods splits there.
        let archetype = format!("Param_{}_{}", function.name.replace("__", ""), param.name);
        archetypes.push(param_archetype(&archetype, &bounds, scope.traits, param.span)?);
        function.generic.push(GenericParam {
            name: param.name.clone(),
            bounds,
            archetype,
        });
    }
    Ok(())
}

/// A type parameter's archetype: no fields, a stub for each member of its
/// bounds' lineages, conforming to all of them.
fn param_archetype(
    name: &str,
    bounds: &[TraitRef],
    traits: &HashMap<TraitRef, TraitEntry>,
    span: Span,
) -> Result<TypeDef> {
    let mut lineage_all: Vec<TraitRef> = Vec::new();
    for bound in bounds {
        for each in lineage(bound, traits) {
            if !lineage_all.contains(&each) {
                lineage_all.push(each);
            }
        }
    }
    let mut methods: Vec<Method> = Vec::new();
    let mut owner: HashMap<String, TraitRef> = HashMap::new();
    for each in &lineage_all {
        let members = visible_members(each, traits);
        for member in &traits[each].def.members {
            if let Some(other) = owner.get(&member.name) {
                return Err(Error::at(
                    format!(
                        "`{}` is a member of both `{}` and `{}`, so a type parameter cannot have both",
                        member_display(&member.name),
                        other.name,
                        each.name
                    ),
                    span,
                ));
            }
            owner.insert(member.name.clone(), each.clone());
            methods.push(provided_method(member, stub_body(span), each, members.clone()));
        }
    }
    Ok(TypeDef {
        exported: false,
        name: name.to_string(),
        name_span: span,
        type_params: Vec::new(),
        params: Vec::new(),
        fields: Vec::new(),
        methods,
        routes: Vec::new(),
        conversions: Vec::new(),
        union_members: None,
        traits: lineage_all,
        conditional: Vec::new(),
        derived: Vec::new(),
    })
}

/// `loop {}`: a body of any return type that never returns.
fn stub_body(span: Span) -> Block {
    let empty = Block {
        items: Vec::new(),
        tail: None,
        span,
    };
    Block {
        items: Vec::new(),
        tail: Some(Box::new(Expr {
            kind: ExprKind::Loop { body: empty },
            span,
        })),
        span,
    }
}

fn void_type(span: Span) -> TypeExpr {
    TypeExpr {
        name: "Void".into(),
        args: Vec::new(),
        span,
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

/// A type written in `module`, spelled so two modules' spellings compare: a
/// declared type by its module, `Self` as the conforming type.
fn canonical(ty: &TypeExpr, module: &str, target: &(String, String), scope: &Scope) -> String {
    render(ty, &|name: &str| {
        if name == "Self" {
            format!("{}::{}", target.0, target.1)
        } else if let Some((from, original)) = scope.type_named(module, name) {
            format!("{from}::{original}")
        } else {
            name.to_string()
        }
    })
}

/// A trait member's signature as the conforming type sees it.
fn signature(member: &TraitMember, target: &str) -> String {
    let name = |name: &str| {
        if name == "Self" {
            target.to_string()
        } else {
            name.to_string()
        }
    };
    match accessor_of(&member.name) {
        Some((AccessorKind::Get, property)) => {
            let ty = member.return_type.as_ref().map_or("Void".to_string(), |ty| render(ty, &name));
            return format!("get {property}: {ty}");
        }
        Some((AccessorKind::Set, property)) => {
            let ty = member.params.first().map_or(String::new(), |param| render(&param.ty, &name));
            return format!("set {property}(value: {ty})");
        }
        None => {}
    }
    let params: Vec<String> = member
        .params
        .iter()
        .map(|param| {
            format!(
                "{}{}{}: {}",
                if param.mutable { "mut " } else { "" },
                param.name,
                if param.positional { "!" } else { "" },
                render(&param.ty, &name)
            )
        })
        .collect();
    let mut text = format!("{}({})", member.name, params.join(", "));
    if let Some(ret) = &member.return_type {
        text.push_str(&format!(" -> {}", render(ret, &name)));
    }
    text
}

fn render(ty: &TypeExpr, name: &dyn Fn(&str) -> String) -> String {
    let mut text = if let Some(members) = &ty.union_members {
        let parts: Vec<String> = members.iter().map(|member| render(member, name)).collect();
        format!("({})", parts.join(" | "))
    } else if let Some(lit) = &ty.lit {
        match lit {
            TypeLit::Int(value) => value.to_string(),
            TypeLit::Bool(value) => value.to_string(),
            TypeLit::String(value) => format!("{value:?}"),
        }
    } else if let Some(fn_type) = &ty.fn_type {
        let params: Vec<String> = fn_type
            .params
            .iter()
            .map(|(label, param)| format!("{label}: {}", render(param, name)))
            .collect();
        format!("({}) -> {}", params.join(", "), render(&fn_type.ret, name))
    } else {
        let mut text = name(&ty.name);
        if !ty.args.is_empty() {
            let args: Vec<String> = ty
                .args
                .iter()
                .map(|arg| match arg {
                    TypeArg::Type(inner) => render(inner, name),
                    TypeArg::Int { value, .. } => value.to_string(),
                })
                .collect();
            text.push_str(&format!("<{}>", args.join(", ")));
        }
        text
    };
    match ty.mark {
        Some(TypeMark::Extract) => text.push('!'),
        Some(TypeMark::Residual) => text.push('?'),
        None => {}
    }
    if let Some(clause) = &ty.where_clause {
        text.push_str(&format!(" where {}", render_where(&clause.expr)));
    }
    text
}

fn render_where(expr: &WhereExpr) -> String {
    match expr {
        WhereExpr::And(left, right) => format!("({} & {})", render_where(left), render_where(right)),
        WhereExpr::Or(left, right) => format!("({} | {})", render_where(left), render_where(right)),
        WhereExpr::Atom(atom) => format!("{:?} {:?}", atom.pred, atom.message),
    }
}
