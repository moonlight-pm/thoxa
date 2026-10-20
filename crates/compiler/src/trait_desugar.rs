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
    assoc: Vec<AssocEntry>,
}

/// A trait's associated type (D228 §7), its bounds resolved in the trait's
/// module.
#[derive(Clone)]
struct AssocEntry {
    name: String,
    bounds: Vec<TraitRef>,
    /// `type Item = Cursor.Item`: the path the trait fixes it as.
    alias: Option<Vec<String>>,
}

/// An associated type a value of some bounds has, nested (`Item`, `Cursor`,
/// `Cursor.Item`): its bounds, a `where` clause's among them, or the path the
/// trait fixes it as.
struct AssocNode {
    path: String,
    bounds: Vec<TraitRef>,
    alias: Option<String>,
}

/// How deep associated types nest (`S.Cursor.Item` is two).
const ASSOC_DEPTH: usize = 4;

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
    /// The entry module's id: its types' C names are bare.
    entry: String,
    /// Each generic type's parameters.
    params: HashMap<(String, String), Vec<String>>,
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

    /// The declared type a MIR name (`mangle`'s) names.
    fn unmangle(&self, mir: &str) -> Option<(String, String)> {
        self.types
            .iter()
            .find(|(module, name)| self.mangle(module, name) == mir)
            .cloned()
    }

    /// A name in a type `globalize` wrote, as its source spells it.
    fn source_name(&self, name: &str) -> String {
        let mir = name.strip_prefix(RESOLVED_TYPE).unwrap_or(name);
        match self.unmangle(mir) {
            Some((_, original)) if mir != name || mir != original => original,
            _ => name.to_string(),
        }
    }

    /// A declared type's name in the MIR, which resolves in any module.
    fn mangle(&self, module: &str, name: &str) -> String {
        if module == self.entry {
            name.to_string()
        } else {
            format!("{module}_{name}")
        }
    }

    /// `ty`, written in `module`, with its declared types (and `Self`, as
    /// `target`) named by their MIR names, so it means the same anywhere.
    fn globalize(&self, ty: &TypeExpr, module: &str, target: &(String, String)) -> TypeExpr {
        let mut out = ty.clone();
        // A plain name is marked resolved; a generic one is found by its
        // MIR name, which no source name in another module spells.
        let mark = |name: String| if ty.args.is_empty() { format!("{RESOLVED_TYPE}{name}") } else { name };
        if ty.name == "Self" {
            out.name = mark(self.mangle(&target.0, &target.1));
        } else if ty.name != MIXED {
            if let Some((from, original)) = self.type_named(module, &ty.name) {
                out.name = mark(self.mangle(&from, &original));
            }
        }
        for arg in &mut out.args {
            if let TypeArg::Type(inner) = arg {
                *inner = self.globalize(inner, module, target);
            }
        }
        if let Some(members) = &mut out.union_members {
            for member in members.iter_mut() {
                *member = self.globalize(member, module, target);
            }
        }
        if let Some(fn_type) = &mut out.fn_type {
            for (_, param) in &mut fn_type.params {
                *param = self.globalize(param, module, target);
            }
            fn_type.ret = self.globalize(&fn_type.ret, module, target);
        }
        out
    }
}

/// Conformances of built-in types, by the type's name (`Int64`, `Array`),
/// each holding for the instances whose arguments meet its bounds.
pub type BuiltInConformances = HashMap<String, Vec<ConditionalConformance>>;

/// Built-in types' associated types, by the type's name (`Array`): each with
/// its conformance's parameters, which it is written with (`type Cursor =
/// ArrayCursor<T>`), in MIR names, and its bounds (D228 §7).
pub type BuiltInAssoc = HashMap<String, Vec<(Vec<String>, String, TypeExpr, Vec<TraitRef>)>>;

/// What the trait pass tells the MIR besides the AST it rewrites.
#[derive(Default)]
pub struct TraitOutput {
    pub built_in: BuiltInConformances,
    pub built_in_assoc: BuiltInAssoc,
    /// `mixed T` as a module spells it, (module id, trait name), → the mangled
    /// name of the union of `T`'s conforming types (D228 §6).
    pub mixed: HashMap<(String, String), String>,
    /// Each such union's members whose signatures name an associated type,
    /// which differs from one conforming type to the next (D228 §7).
    pub mixed_closed: HashMap<String, HashSet<String>>,
    /// Each such union's trait and the members of its lineage, whose
    /// signatures calls through it take (D235): the trait's module, its
    /// name, and the members.
    pub mixed_members: HashMap<String, (TraitRef, Vec<TraitMember>)>,
}

pub fn desugar_package(package: &mut Package) -> Result<TraitOutput> {
    let has_traits = package.modules.iter().any(|module| {
        module
            .ast
            .items
            .iter()
            .any(|item| matches!(item, Item::Trait(_) | Item::Conform(_)))
    });
    if !has_traits {
        return Ok(TraitOutput::default());
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
                    assoc: Vec::new(),
                },
            );
        }
    }

    let entry_module = package
        .modules
        .iter()
        .find(|module| module.is_entry)
        .map(|module| module.id.clone())
        .unwrap_or_default();
    // Parents and associated types' bounds, resolved in the trait's module.
    for key in &keys {
        let entry = &traits[key];
        let mut parents = Vec::new();
        let scope = Scope {
            imports: &imports,
            types: HashSet::new(),
            traits: &traits,
            core_types: HashSet::new(),
            entry: entry_module.clone(),
            params: HashMap::new(),
        };
        let mut assoc = Vec::new();
        for declared in &entry.def.assoc {
            let mut bounds = Vec::new();
            for (name, span) in &declared.bounds {
                match scope.trait_named(&key.module, name) {
                    Some(bound) => bounds.push(bound),
                    None => errors.push(
                        Error::at(format!("unknown trait `{name}`"), *span).with_path(entry.path.clone()),
                    ),
                }
            }
            let alias = match &declared.alias {
                None => None,
                Some(ty) if !bounds.is_empty() => {
                    errors.push(
                        Error::at(
                            format!("`{}` is fixed as `{}`, so it takes no bounds of its own", declared.name, ty.name),
                            declared.span,
                        )
                        .with_path(entry.path.clone()),
                    );
                    None
                }
                Some(ty) if ty.args.is_empty() && ty.lit.is_none() && ty.union_members.is_none() && ty.fn_type.is_none() => {
                    Some(ty.name.split('.').map(str::to_string).collect())
                }
                Some(ty) => {
                    errors.push(
                        Error::at(
                            format!(
                                "a trait fixes `{}` as one of its associated types (`type {} = Cursor.Item`)",
                                declared.name, declared.name
                            ),
                            ty.span,
                        )
                        .with_path(entry.path.clone()),
                    );
                    None
                }
            };
            assoc.push(AssocEntry {
                name: declared.name.clone(),
                bounds,
                alias,
            });
        }
        for (name, span) in &entry.def.parents {
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
        let entry = traits.get_mut(key).unwrap();
        entry.parents = parents;
        entry.assoc = assoc;
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
        let scope = Scope {
            imports: &imports,
            types: HashSet::new(),
            traits: &traits,
            core_types: HashSet::new(),
            entry: entry_module.clone(),
            params: HashMap::new(),
        };
        match archetype(key, &scope) {
            Ok(defs) => {
                if let Some(module) = package.modules.iter_mut().find(|module| module.id == key.module) {
                    module.ast.items.extend(defs.into_iter().map(Item::Type));
                }
            }
            Err(error) => errors.push(error.with_path(traits[key].path.clone())),
        }
    }
    if !errors.is_empty() {
        return Err(Error::multi(errors));
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
    let params = package
        .modules
        .iter()
        .flat_map(|module| {
            module.ast.items.iter().filter_map(|item| match item {
                Item::Type(def) if !def.type_params.is_empty() => {
                    Some(((module.id.clone(), def.name.clone()), def.type_params.clone()))
                }
                _ => None,
            })
        })
        .collect();
    let scope = Scope {
        imports: &imports,
        types,
        traits: &traits,
        core_types,
        entry: entry_module,
        params,
    };
    generic_functions(package, &scope)?;
    type_bounds(package, &scope)?;
    // The type's own members, before any conformance adds to them.
    let mut own_members: HashMap<(String, String), OwnMembers> = HashMap::new();
    let mut generic: HashMap<(String, String), Vec<String>> = HashMap::new();
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
                generic.insert(key, def.type_params.clone());
            }
        }
    }

    // A generic type's conformance writes its members and associated types
    // with its own names for the type's parameters (`conform Box<U: …>`):
    // they become the type's (`T`), as its methods write them.
    for item in &mut pending {
        let Some(params) = scope
            .type_named(&item.module, &item.conformance.ty.name)
            .and_then(|target| generic.get(&target))
        else {
            continue;
        };
        let written: Vec<&str> = item.conformance.params.iter().map(|param| param.name.as_str()).collect();
        let distinct: HashSet<&str> = written.iter().copied().collect();
        // A wrong count or a name twice is the conformance's error.
        if written.len() != params.len() || distinct.len() != written.len() || written == *params {
            continue;
        }
        let renames: HashMap<String, TypeExpr> = written
            .iter()
            .zip(params)
            .map(|(from, to)| {
                (
                    from.to_string(),
                    TypeExpr {
                        name: to.clone(),
                        ..void_type(item.conformance.span)
                    },
                )
            })
            .collect();
        let conformance = &mut item.conformance;
        for (param, name) in conformance.params.iter_mut().zip(params) {
            param.name = name.clone();
        }
        for method in &mut conformance.methods {
            *method = crate::mir::subst_method(method, &renames);
        }
        for (_, _, ty) in &mut conformance.assoc {
            *ty = crate::mir::subst_type_expr(ty, &renames);
        }
    }

    // A built-in type's own methods are std's `extend` methods for it (D196).
    for module in &package.modules {
        for item in &module.ast.items {
            let Item::Function(function) = item else { continue };
            for (name, _) in CONFORMABLE_BUILT_INS {
                let prefix = crate::extend_desugar::extension_fn_name(name, "");
                let Some(member) = function.name.strip_prefix(&prefix) else { continue };
                let method = Method {
                    exported: function.exported,
                    name: member.to_string(),
                    name_span: function.name_span,
                    type_params: Vec::new(),
                    params: function.params.iter().skip(1).cloned().collect(),
                    return_type: function.return_type.clone(),
                    body: function.body.clone(),
                    provided: None,
                    requires: Vec::new(),
                    generic: Vec::new(),
                };
                own_members
                    .entry((BUILT_IN_MODULE.to_string(), name.to_string()))
                    .or_default()
                    .methods
                    .insert(member.to_string(), method);
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

    // Associated types, named before any conformance is checked: a member
    // may write another type's (`Cursor.Item`).
    let mut named: Named = HashMap::new();
    let mut listed: Listed = HashMap::new();
    for item in &pending {
        let name = &item.conformance.ty.name;
        let built_in = CONFORMABLE_BUILT_INS.iter().any(|(each, _)| each == name);
        let Some(target) = (if built_in {
            Some((BUILT_IN_MODULE.to_string(), name.clone()))
        } else {
            scope.type_named(&item.module, name)
        }) else {
            continue;
        };
        let names = named.entry(target.clone()).or_default();
        let mut here = HashSet::new();
        for (name, span, ty) in &item.conformance.assoc {
            // Twice in one conformance is that conformance's error.
            if !here.insert(name.as_str()) {
                continue;
            }
            if names.insert(name.clone(), (ty.clone(), item.module.clone())).is_some() {
                errors.push(
                    Error::at(format!("`{}` names `type {name}` in two conformances", target.1), *span)
                        .with_path(item.path.clone()),
                );
            }
        }
        for (name, _) in &item.conformance.traits {
            if let Some(trait_ref) = scope.trait_named(&item.module, name) {
                listed.entry(target.clone()).or_default().push(trait_ref);
            }
        }
    }
    let mut assoc_types: HashMap<(String, String), Vec<(String, TypeExpr, Vec<TraitRef>)>> = HashMap::new();
    let mut extensions: Vec<(String, Function, HashMap<String, Vec<TraitRef>>, PathBuf)> = Vec::new();
    let mut built_in_assoc: BuiltInAssoc = HashMap::new();

    for item in &pending {
        let result = check_conformance(
            item,
            &scope,
            &own_members,
            &generic,
            &unions,
            &mut conformed,
            &mut trait_members,
            &named,
            &listed,
        );
        match result {
            Ok((target, refs, methods, defaults, entries)) if target.0 == BUILT_IN_MODULE && !scope.types.contains(&target) => {
                // A built-in type's members are `extend` functions, each in
                // the module its body resolves in; its associated types go to
                // the MIR by the built-in's name.
                let bounds: HashMap<String, Vec<TraitRef>> = item
                    .conformance
                    .params
                    .iter()
                    .map(|param| param.name.clone())
                    .zip(refs.first().and_then(|first| conformed[&target][first].1.clone()).unwrap_or_default())
                    .collect();
                for method in &methods {
                    let (module, function) = extension_function(method, &item.conformance, &item.module);
                    extensions.push((module, function, bounds.clone(), item.path.clone()));
                }
                let params: Vec<String> = item.conformance.params.iter().map(|param| param.name.clone()).collect();
                built_in_assoc
                    .entry(target.1.clone())
                    .or_default()
                    .extend(entries.into_iter().map(|(name, ty, bounds)| (params.clone(), name, ty, bounds)));
                let _ = defaults;
            }
            Ok((target, refs, methods, defaults, entries)) => {
                assoc_types.entry(target.clone()).or_default().extend(entries);
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

    for (module_id, mut function, bounds, path) in extensions {
        let Some(module) = package.modules.iter_mut().find(|module| module.id == module_id) else { continue };
        let mut archetypes = Vec::new();
        if let Err(error) = resolve_generics(&mut function, &module_id, &scope, &mut archetypes, &bounds) {
            errors.push(error.with_path(path));
            continue;
        }
        module.ast.items.push(Item::Function(function));
        module.ast.items.extend(archetypes.into_iter().map(Item::Type));
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
            if let Some(entries) = assoc_types.remove(&key) {
                def.assoc = entries;
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
    checking_instances(package, &scope)?;
    let (mixed, mixed_closed, mixed_members) = mixed_unions(package, &scope, &conformed, &traits)?;
    Ok(TraitOutput {
        built_in,
        built_in_assoc,
        mixed,
        mixed_closed,
        mixed_members,
    })
}

/// `mixed T` (D228 §6): a union, in `T`'s module, of the types that conform
/// to `T` whose values convert into it, which the MIR adds as it meets them
/// (D235): declared types, a generic type's instances, built-in types. Its
/// member calls dispatch on the member; it conforms to `T` and `T`'s
/// ancestors.
type MixedUnions = (
    HashMap<(String, String), String>,
    HashMap<String, HashSet<String>>,
    HashMap<String, (TraitRef, Vec<TraitMember>)>,
);

fn mixed_unions(
    package: &mut Package,
    scope: &Scope,
    conformed: &Conformed,
    traits: &HashMap<TraitRef, TraitEntry>,
) -> Result<MixedUnions> {
    let mut uses: Vec<(String, String, TraitRef, Span, PathBuf)> = Vec::new();
    for module in &package.modules {
        let mut apps = Vec::new();
        for item in &module.ast.items {
            crate::mir::collect_item_type_apps(item, &mut apps);
        }
        for (te, span) in apps {
            if te.name != MIXED {
                continue;
            }
            let Some(TypeArg::Type(arg)) = te.args.first() else { continue };
            let Some(trait_ref) = scope.trait_named(&module.id, &arg.name) else {
                return Err(Error::at(format!("unknown trait `{}`", arg.name), arg.span)
                    .with_path(module.path.clone()));
            };
            uses.push((module.id.clone(), arg.name.clone(), trait_ref, span, module.path.clone()));
        }
    }
    let mangle = |module: &str, name: &str| -> String {
        let entry = package
            .modules
            .iter()
            .any(|each| each.id == module && each.is_entry);
        if entry {
            name.to_string()
        } else {
            format!("{module}_{name}")
        }
    };
    let mut made: HashMap<TraitRef, String> = HashMap::new();
    let mut closed: HashMap<String, HashSet<String>> = HashMap::new();
    let mut signatures: HashMap<String, (TraitRef, Vec<TraitMember>)> = HashMap::new();
    let mut unions: Vec<(String, TypeDef)> = Vec::new();
    for (_, _, trait_ref, span, path) in &uses {
        if made.contains_key(trait_ref) {
            continue;
        }
        let conforms = conformed.values().any(|done| done.contains_key(trait_ref));
        if !conforms {
            return Err(Error::at(
                format!("no type conforms to `{}`, so `mixed {}` has no values", trait_ref.name, trait_ref.name),
                *span,
            )
            .with_path(path.clone()));
        }
        let name = format!("Mixed_{}", trait_ref.name);
        unions.push((
            trait_ref.module.clone(),
            TypeDef {
                exported: true,
                name: name.clone(),
                name_span: *span,
                type_params: Vec::new(),
                params: Vec::new(),
                fields: Vec::new(),
                methods: Vec::new(),
                routes: Vec::new(),
                conversions: Vec::new(),
                // Its members are the types that convert into it (D235).
                union_members: Some(Vec::new()),
                traits: lineage(trait_ref, traits),
                conditional: Vec::new(),
                derived: Vec::new(),
                param_bounds: Vec::new(),
                bounds: Vec::new(),
                assoc_bounds: Vec::new(),
                mixed: Some(trait_ref.clone()),
                assoc: Vec::new(),
                checking: Vec::new(),
            },
        ));
        made.insert(trait_ref.clone(), mangle(&trait_ref.module, &name));
        let mut nodes = Vec::new();
        assoc_tree(std::slice::from_ref(trait_ref), "", &HashMap::new(), traits, *span, &mut nodes)
            .map_err(|error| error.with_path(path.clone()))?;
        let assoc: Vec<&str> = nodes.iter().map(|node| node.path.as_str()).collect();
        let names_assoc = |ty: &TypeExpr| mentions_name(ty, &assoc);
        let members: HashSet<String> = lineage(trait_ref, traits)
            .iter()
            .flat_map(|each| traits[each].def.members.iter())
            .filter(|member| {
                member.params.iter().any(|param| names_assoc(&param.ty))
                    || member.return_type.as_ref().is_some_and(names_assoc)
            })
            .map(|member| member.name.clone())
            .collect();
        closed.insert(mangle(&trait_ref.module, &name), members);
        let lineage_members = lineage(trait_ref, traits)
            .iter()
            .flat_map(|each| traits[each].def.members.iter().cloned())
            .collect();
        signatures.insert(mangle(&trait_ref.module, &name), (trait_ref.clone(), lineage_members));
    }
    for (module_id, def) in unions {
        if let Some(module) = package.modules.iter_mut().find(|module| module.id == module_id) {
            module.ast.items.push(Item::Type(def));
        }
    }
    let mixed = uses
        .into_iter()
        .map(|(module, name, trait_ref, _, _)| ((module, name), made[&trait_ref].clone()))
        .collect();
    Ok((mixed, closed, signatures))
}

/// Whether `ty` names one of `names` (or a path under one: `Cursor.Item`).
fn mentions_name(ty: &TypeExpr, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| ty.name == *name || ty.name.strip_prefix(name).is_some_and(|rest| rest.starts_with('.')))
        || ty.args.iter().any(|arg| matches!(arg, TypeArg::Type(inner) if mentions_name(inner, names)))
        || ty.union_members.iter().flatten().any(|member| mentions_name(member, names))
        || ty.fn_type.as_ref().is_some_and(|fn_type| {
            fn_type.params.iter().any(|(_, param)| mentions_name(param, names)) || mentions_name(&fn_type.ret, names)
        })
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

/// The members the runtime gives a built-in type, which a conformance cannot
/// add again (`get_count` is the property `count`).
const BUILT_IN_PRIMITIVES: &[(&str, &[&str])] = &[
    ("Array", &["append", "dropLast", "insert", "removeAt", "get_count"]),
    ("Map", &["has", "get", "set", "remove", "keys", "values", "get_count"]),
    ("String", &["byteAt", "byteSlice", "byteIndexOf", "get_count", "get_byteCount"]),
    ("Bytes", &["get", "set", "slice", "find", "equals", "insert", "removeAt", "get_count"]),
    ("Buffer", &["append", "appendBytes", "freeze", "get_count"]),
];

/// Traits whose required members the compiler implements for a conformance
/// without them, field by field (D228 §4).
fn has_defaults(trait_ref: &TraitRef) -> bool {
    (trait_ref.module == CORE_MODULE
        && matches!(trait_ref.name.as_str(), "Equatable" | "Hashable" | "Ordered" | "Show"))
        || (trait_ref.module == JSON_MODULE && trait_ref.name == "Encodable")
}

/// std/json, whose `Encodable` default is the generated encoder (D182).
const JSON_MODULE: &str = "std_json";

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
    let mut seen = HashSet::new();
    for assoc in &def.assoc {
        if !seen.insert(assoc.name.as_str()) {
            return Err(Error::at(
                format!("`type {}` is declared twice in `{}`", assoc.name, def.name),
                assoc.span,
            ));
        }
    }
    Ok(())
}

/// The associated types a value bounded by `bounds` has (D228 §7), nested,
/// parents before children and the ones a trait fixes after the rest; at
/// `prefix`, with a `where` clause's bounds (`extra`, by path) added.
fn assoc_tree(
    bounds: &[TraitRef],
    prefix: &str,
    extra: &HashMap<String, Vec<TraitRef>>,
    traits: &HashMap<TraitRef, TraitEntry>,
    span: Span,
    out: &mut Vec<AssocNode>,
) -> Result<()> {
    let mut direct: Vec<AssocEntry> = Vec::new();
    for bound in bounds {
        for each in lineage(bound, traits) {
            for assoc in &traits[&each].assoc {
                match direct.iter_mut().find(|seen| seen.name == assoc.name) {
                    Some(seen) if seen.alias != assoc.alias => {
                        return Err(Error::at(
                            format!("two of the bounds declare `type {}` differently", assoc.name),
                            span,
                        ))
                    }
                    Some(seen) => {
                        for bound in &assoc.bounds {
                            if !seen.bounds.contains(bound) {
                                seen.bounds.push(bound.clone());
                            }
                        }
                    }
                    None => direct.push(assoc.clone()),
                }
            }
        }
    }
    if !direct.is_empty() && prefix.split('.').count() >= ASSOC_DEPTH {
        return Err(Error::at(
            format!("associated types nest too deep at `{prefix}` (does a bound lead back to its own trait?)"),
            span,
        ));
    }
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        }
    };
    for assoc in direct.iter().filter(|assoc| assoc.alias.is_none()) {
        let path = join(&assoc.name);
        let mut bounds = assoc.bounds.clone();
        for bound in extra.get(&path).into_iter().flatten() {
            if !bounds.contains(bound) {
                bounds.push(bound.clone());
            }
        }
        out.push(AssocNode {
            path: path.clone(),
            bounds: bounds.clone(),
            alias: None,
        });
        assoc_tree(&bounds, &path, extra, traits, span, out)?;
    }
    for assoc in direct.iter().filter(|assoc| assoc.alias.is_some()) {
        out.push(AssocNode {
            path: join(&assoc.name),
            bounds: Vec::new(),
            alias: Some(join(&assoc.alias.as_ref().unwrap().join("."))),
        });
    }
    Ok(())
}

/// `path` with every fixed associated type along it followed: `Item` is
/// `Cursor.Item` in a `Sequence`.
fn unalias(path: &str, nodes: &[AssocNode]) -> String {
    let mut path = path.to_string();
    for _ in 0..=nodes.len() {
        let segments: Vec<&str> = path.split('.').collect();
        let found = (1..=segments.len()).find_map(|count| {
            let prefix = segments[..count].join(".");
            nodes
                .iter()
                .find(|node| node.path == prefix && node.alias.is_some())
                .map(|node| (count, node.alias.clone().unwrap()))
        });
        let Some((count, target)) = found else { break };
        let rest = &segments[count..];
        path = if rest.is_empty() {
            target
        } else {
            format!("{target}.{}", rest.join("."))
        };
    }
    path
}

/// The archetypes for a value of some bounds and its associated types
/// (`Param_count_I`, `Param_count_I_Item`): each a type with a stub for every
/// member of its bounds' lineages, its members' associated types named by the
/// archetypes'.
struct Archetypes<'a> {
    base: String,
    module: String,
    nodes: Vec<AssocNode>,
    scope: &'a Scope<'a>,
    /// The ones a `where` fixes (D231), by path: the type itself, not an
    /// archetype.
    fixed: HashMap<String, TypeExpr>,
}

impl Archetypes<'_> {
    /// The archetype's name for `path` ("" for the value's own).
    fn name(&self, path: &str) -> String {
        let path = unalias(path, &self.nodes);
        if path.is_empty() {
            self.base.clone()
        } else {
            format!("{}_{}", self.base, path.replace('.', "_"))
        }
    }

    fn type_expr(&self, path: &str, span: Span) -> TypeExpr {
        if let Some(fixed) = self.fixed.get(&unalias(path, &self.nodes)) {
            return TypeExpr { span, ..fixed.clone() };
        }
        TypeExpr {
            name: format!("{RESOLVED_TYPE}{}", self.scope.mangle(&self.module, &self.name(path))),
            ..void_type(span)
        }
    }

    /// What the associated types under `at` are, by their paths from it.
    fn subst(&self, at: &str, span: Span) -> HashMap<String, TypeExpr> {
        self.nodes
            .iter()
            .filter_map(|node| {
                let relative = if at.is_empty() {
                    Some(node.path.as_str())
                } else {
                    node.path.strip_prefix(at).and_then(|rest| rest.strip_prefix('.'))
                }?;
                Some((relative.to_string(), self.type_expr(&node.path, span)))
            })
            .collect()
    }

    /// The bounds of the associated type at `path`, through what fixes it.
    fn bounds(&self, path: &str) -> Vec<TraitRef> {
        let path = unalias(path, &self.nodes);
        self.nodes
            .iter()
            .find(|node| node.path == path)
            .map(|node| node.bounds.clone())
            .unwrap_or_default()
    }

    /// The associated types directly under `at`, for its `TypeDef::assoc`.
    fn entries(&self, at: &str, span: Span) -> Vec<(String, TypeExpr, Vec<TraitRef>)> {
        self.nodes
            .iter()
            .filter_map(|node| {
                let name = if at.is_empty() {
                    node.path.as_str()
                } else {
                    node.path.strip_prefix(at)?.strip_prefix('.')?
                };
                (!name.contains('.')).then(|| (name.to_string(), self.type_expr(&node.path, span), self.bounds(&node.path)))
            })
            .collect()
    }

    /// The associated types' archetypes, stubs only.
    fn assoc_defs(&self, span: Span) -> Result<Vec<TypeDef>> {
        let mut defs = Vec::new();
        for node in self.nodes.iter().filter(|node| node.alias.is_none() && !self.fixed.contains_key(&node.path)) {
            defs.push(param_archetype(
                &self.name(&node.path),
                &node.bounds,
                self.scope.traits,
                span,
                &self.subst(&node.path, span),
                self.entries(&node.path, span),
            )?);
        }
        Ok(defs)
    }
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

/// A checked conformance: its target, its traits, the methods it adds, which
/// of them are the compiler's defaults, and the associated types it names.
type Checked = ((String, String), Vec<TraitRef>, Vec<Method>, Vec<String>, Vec<(String, TypeExpr, Vec<TraitRef>)>);

/// Each type's associated types as its conformances name them, before any
/// conformance is checked (D228 §7): the type as written and its module.
type Named = HashMap<(String, String), HashMap<String, (TypeExpr, String)>>;

/// The traits each type's conformances list.
type Listed = HashMap<(String, String), Vec<TraitRef>>;

/// A conformance's associated types, by their paths (`Item`, `Cursor.Item`):
/// as the MIR names them, and spelled to compare.
#[derive(Default)]
struct ConformedAssoc {
    types: HashMap<String, TypeExpr>,
    canonical: HashMap<String, String>,
    /// As the conformance writes them, for messages.
    shown: HashMap<String, String>,
}

/// The type at `path` under `target`'s associated types, as written, its
/// module, and the type `Self` means there.
fn assoc_of(
    target: &(String, String),
    path: &str,
    named: &Named,
    listed: &Listed,
    scope: &Scope,
    depth: usize,
) -> Option<(TypeExpr, String, (String, String))> {
    if depth > ASSOC_DEPTH * 2 {
        return None;
    }
    let (first, rest) = match path.split_once('.') {
        Some((first, rest)) => (first, Some(rest)),
        None => (path, None),
    };
    let Some((ty, module)) = named.get(target).and_then(|names| names.get(first)) else {
        // One a trait fixes as another (`Item` is `Cursor.Item`).
        let alias = listed
            .get(target)?
            .iter()
            .flat_map(|trait_ref| lineage(trait_ref, scope.traits))
            .find_map(|each| {
                scope.traits[&each]
                    .assoc
                    .iter()
                    .find(|assoc| assoc.name == first)
                    .and_then(|assoc| assoc.alias.clone())
            })?;
        let mut full = alias.join(".");
        if let Some(rest) = rest {
            full = format!("{full}.{rest}");
        }
        return assoc_of(target, &full, named, listed, scope, depth + 1);
    };
    let Some(rest) = rest else {
        return Some((ty.clone(), module.clone(), target.clone()));
    };
    let inner = if ty.name == "Self" {
        target.clone()
    } else {
        scope.type_named(module, &ty.name)?
    };
    let found = assoc_of(&inner, rest, named, listed, scope, depth + 1)?;
    if ty.args.is_empty() {
        return Some(found);
    }
    // A generic type's conformance writes it with the type's parameters
    // (`type Item = T`), which are this type's arguments here; both are
    // named as the MIR names them, so it means the same in either module.
    let (found, found_module, owner) = found;
    let args: HashMap<String, TypeExpr> = scope
        .params
        .get(&inner)?
        .iter()
        .zip(&ty.args)
        .filter_map(|(param, arg)| match arg {
            TypeArg::Type(arg) => Some((param.clone(), scope.globalize(arg, module, target))),
            TypeArg::Int { .. } => None,
        })
        .collect();
    let found = crate::mir::subst_type_expr(&scope.globalize(&found, &found_module, &owner), &args);
    Some((found, found_module, owner))
}

#[allow(clippy::too_many_arguments)]
fn check_conformance(
    item: &Pending,
    scope: &Scope,
    own_members: &HashMap<(String, String), OwnMembers>,
    generic: &HashMap<(String, String), Vec<String>>,
    unions: &HashSet<(String, String)>,
    conformed: &mut Conformed,
    trait_members: &mut HashMap<(String, String), HashMap<String, TraitRef>>,
    named: &Named,
    listed: &Listed,
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
        let arity = generic.get(&target).map(Vec::len);
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
    // A built-in type's members are functions of the module they are written
    // in, so they may be written in the trait's.
    if !conformance.methods.is_empty() && item.module != target.0 && built_in.is_none() {
        return Err(Error::at(
            format!(
                "a conformance with members goes in the module that defines `{}` (members in the trait's module are not supported yet)",
                target.1
            ),
            conformance.span,
        ));
    }

    let mut assoc = conformance_assoc(item, &target, &refs, named, listed, scope)?;
    // A generic type's `Self` is its instance: `Box<T>`, with the type's own
    // parameters (the conformance's were renamed to them); a built-in's is
    // the built-in, with the conformance's (`Array<T>`).
    let params: Vec<String> = match built_in {
        Some(_) => conformance.params.iter().map(|param| param.name.clone()).collect(),
        None => generic.get(&target).cloned().unwrap_or_default(),
    };
    if built_in.is_some() || !params.is_empty() {
        let args = if params.is_empty() { String::new() } else { format!("<{}>", params.join(", ")) };
        let canonical_self = match built_in {
            Some(_) => format!("{}{args}", target.1),
            None => format!("{}::{}{args}", target.0, target.1),
        };
        assoc.canonical.insert("Self".into(), canonical_self);
        assoc.shown.insert("Self".into(), format!("{}{args}", target.1));
        for param in &params {
            assoc.canonical.insert(param.clone(), param.clone());
        }
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
            let primitive = built_in.is_some()
                && BUILT_IN_PRIMITIVES
                    .iter()
                    .any(|(name, members)| *name == target.1 && members.contains(&member.name.as_str()));
            if primitive && (implemented.contains_key(member.name.as_str()) || member.body.is_some()) {
                return Err(clash());
            }
            match (implemented.get(member.name.as_str()), &member.body) {
                (Some(_), _) if own_method.is_some() => return Err(clash()),
                (Some(method), _) => methods.push(implementation(
                    method,
                    member,
                    trait_ref,
                    item,
                    &target,
                    scope,
                    &assoc,
                )?),
                // A required accessor the type's own accessor or field
                // satisfies: uniform access.
                (None, None) if property.is_some() && own_method.is_some() => {
                    let own_item = Pending {
                        module: target.0.clone(),
                        path: item.path.clone(),
                        conformance: item.conformance.clone(),
                    };
                    implementation(own_method.unwrap(), member, trait_ref, &own_item, &target, scope, &assoc)?;
                }
                (None, None) if stored.is_some() => {
                    let (name, field_ty) = stored.unwrap();
                    let declared = match property.unwrap().0 {
                        AccessorKind::Get => member.return_type.as_ref(),
                        AccessorKind::Set => member.params.first().map(|param| &param.ty),
                    };
                    let matches = declared.is_some_and(|declared| {
                        canonical(field_ty, &target.0, &target, scope, &HashMap::new())
                            == canonical(declared, &trait_ref.module, &target, scope, &assoc.canonical)
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
                // std writes a built-in's members with `extend` and conforms
                // it without a body: its own method is the member, with the
                // trait's signature (std's own traits' members may add
                // defaulted options, `String.equals(other, ignoreCase:)`).
                (None, None) if own_method.is_some() && built_in.is_some() => {
                    if !has_defaults(trait_ref) {
                        implementation(own_method.unwrap(), member, trait_ref, item, &target, scope, &assoc)?;
                    }
                }
                (None, None) if own_method.is_some() => return Err(clash()),
                // The compiler's field-by-field default (D228 §4): a method
                // of a plain product; a union or an instance compares
                // structurally without one.
                (None, None) if has_defaults(trait_ref) => {
                    // The codecs generate `toJson` themselves.
                    if bounds.is_none() && !unions.contains(&target) && trait_ref.module == CORE_MODULE {
                        methods.push(default_member(member, conformance.span));
                        derived.push(member.name.clone());
                    }
                }
                (None, None) => {
                    return Err(Error::at(
                        format!(
                            "`{}` does not implement `{}` from `{}`",
                            target.1,
                            signature(member, &target.1, &assoc.shown),
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
    // A conformance's members and the trait's provided ones see its
    // associated types by name.
    // A generic type's members from a conformance with bounds exist only
    // for the instances that meet them.
    let requires = bounds
        .as_ref()
        .filter(|lists| built_in.is_none() && lists.iter().any(|list| !list.is_empty()))
        .cloned()
        .unwrap_or_default();
    let methods = methods
        .iter()
        .map(|method| Method {
            requires: requires.clone(),
            ..crate::mir::subst_method(method, &assoc.types)
        })
        .collect();
    let mut entries = Vec::new();
    for (trait_ref, _) in &refs {
        for declared in &scope.traits[trait_ref].assoc {
            if let Some(ty) = assoc.types.get(&declared.name) {
                entries.push((declared.name.clone(), ty.clone(), declared.bounds.clone()));
            }
        }
    }
    Ok((
        target,
        refs.into_iter().map(|(trait_ref, _)| trait_ref).collect(),
        methods,
        derived,
        entries,
    ))
}

/// A conformance's associated types (D228 §7): each the listed traits
/// declare, named once, and none they fix; then every one its members can
/// write, nested ones among them.
fn conformance_assoc(
    item: &Pending,
    target: &(String, String),
    refs: &[(TraitRef, Span)],
    named: &Named,
    listed: &Listed,
    scope: &Scope,
) -> Result<ConformedAssoc> {
    let conformance = &item.conformance;
    let listed_names = || -> String {
        let names: Vec<String> = refs.iter().map(|(trait_ref, _)| format!("`{}`", trait_ref.name)).collect();
        names.join(" or ")
    };
    let mut supplied: HashSet<&str> = HashSet::new();
    for (name, span, _) in &conformance.assoc {
        if !supplied.insert(name.as_str()) {
            return Err(Error::at(format!("`type {name}` is named twice"), *span));
        }
        let declaring = refs.iter().find_map(|(trait_ref, _)| {
            scope.traits[trait_ref]
                .assoc
                .iter()
                .find(|assoc| assoc.name == *name)
                .map(|assoc| (trait_ref, assoc))
        });
        match declaring {
            None => {
                return Err(Error::at(
                    format!("`{name}` is not an associated type of {}", listed_names()),
                    *span,
                ))
            }
            Some((trait_ref, assoc)) if assoc.alias.is_some() => {
                return Err(Error::at(
                    format!(
                        "`{}` fixes `{name}` as `{}`, so a conformance does not name it",
                        trait_ref.name,
                        assoc.alias.as_ref().unwrap().join(".")
                    ),
                    *span,
                ))
            }
            Some(_) => {}
        }
    }
    for (trait_ref, _) in refs {
        for assoc in scope.traits[trait_ref].assoc.iter().filter(|assoc| assoc.alias.is_none()) {
            if !supplied.contains(assoc.name.as_str()) {
                return Err(Error::at(
                    format!(
                        "`{}` does not name `{}`'s `type {}`",
                        target.1, trait_ref.name, assoc.name
                    ),
                    conformance.span,
                )
                .with_help(format!("add `type {} = …` to the conformance", assoc.name)));
            }
        }
    }
    let bounds: Vec<TraitRef> = refs.iter().map(|(trait_ref, _)| trait_ref.clone()).collect();
    let mut nodes = Vec::new();
    assoc_tree(&bounds, "", &HashMap::new(), scope.traits, conformance.span, &mut nodes)?;
    let mut out = ConformedAssoc::default();
    for node in &nodes {
        let Some((ty, module, owner)) = assoc_of(target, &node.path, named, listed, scope, 0) else {
            continue;
        };
        out.types.insert(node.path.clone(), scope.globalize(&ty, &module, &owner));
        out.shown.insert(node.path.clone(), render(&ty, &|name: &str| scope.source_name(name)));
        out.canonical
            .insert(node.path.clone(), canonical(&ty, &module, &owner, scope, &HashMap::new()));
    }
    // A type another conformance gives the name a trait fixes must be the
    // one it is fixed as.
    for node in nodes.iter().filter(|node| node.alias.is_some() && !node.path.contains('.')) {
        let fixed = assoc_of(target, node.alias.as_ref().unwrap(), named, listed, scope, 0)
            .map(|(ty, module, owner)| canonical(&ty, &module, &owner, scope, &HashMap::new()));
        if let (Some(fixed), Some(given)) = (fixed, out.canonical.get(&node.path)) {
            if fixed != *given {
                return Err(Error::at(
                    format!(
                        "`{}`'s `{}` is `{given}`, and {} fixes it as `{}`, which is `{fixed}`",
                        target.1,
                        node.path,
                        listed_names(),
                        node.alias.as_ref().unwrap()
                    ),
                    conformance.span,
                ));
            }
        }
    }
    Ok(out)
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
    assoc: &ConformedAssoc,
) -> Result<Method> {
    let in_trait = |ty: &TypeExpr| canonical(ty, &trait_ref.module, target, scope, &assoc.canonical);
    let in_conformance = |ty: &TypeExpr| canonical(ty, &item.module, target, scope, &assoc.canonical);
    let mismatch = || {
        Error::at(
            format!(
                "`{}` must match `{}` in `{}`",
                method.name,
                signature(member, &target.1, &assoc.shown),
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
        type_params: Vec::new(),
        params: member.params.clone(),
        return_type: member.return_type.clone(),
        body,
        provided: Some(Provided {
            module: trait_ref.module.clone(),
            members,
        }),
        requires: Vec::new(),
        generic: Vec::new(),
    }
}

/// A built-in type's conformance member as an `extend` method is (D196): a
/// function `__extend_Array_iterate` whose first parameter is `self`, generic
/// over the conformance's parameters, in the module its body resolves in
/// (the trait's, for a member the trait provides).
fn extension_function(method: &Method, conformance: &Conformance, module: &str) -> (String, Function) {
    let self_ty = TypeExpr {
        name: conformance.ty.name.clone(),
        args: conformance
            .params
            .iter()
            .map(|param| {
                TypeArg::Type(TypeExpr {
                    name: param.name.clone(),
                    ..void_type(param.span)
                })
            })
            .collect(),
        ..void_type(conformance.ty.span)
    };
    let selfed = crate::mir::subst_method(method, &HashMap::from([("Self".to_string(), self_ty.clone())]));
    let module = method.provided.as_ref().map_or(module, |provided| provided.module.as_str());
    let receiver = Param {
        name: "self".into(),
        name_span: method.name_span,
        positional: true,
        // By address, so a member may write `self` (D228).
        mutable: true,
        ty: self_ty,
        default: None,
    };
    let function = Function {
        exported: true,
        name: crate::extend_desugar::extension_fn_name(&conformance.ty.name, &method.name),
        name_span: method.name_span,
        type_params: conformance.params.clone(),
        params: std::iter::once(receiver).chain(selfed.params).collect(),
        return_type: selfed.return_type,
        body: selfed.body,
        generic: Vec::new(),
    };
    (module.to_string(), function)
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
fn archetype(trait_ref: &TraitRef, scope: &Scope) -> Result<Vec<TypeDef>> {
    let traits = scope.traits;
    let entry = &traits[trait_ref];
    if entry.def.members.iter().all(|member| member.body.is_none()) {
        return Ok(Vec::new());
    }
    let span = entry.def.name_span;
    let mut nodes = Vec::new();
    assoc_tree(std::slice::from_ref(trait_ref), "", &HashMap::new(), traits, span, &mut nodes)?;
    let archetypes = Archetypes {
        base: format!("Self_{}", trait_ref.name),
        module: trait_ref.module.clone(),
        nodes,
        scope,
        fixed: HashMap::new(),
    };
    let assoc = archetypes.subst("", span);
    let members = visible_members(trait_ref, traits);
    let lineage = lineage(trait_ref, traits);
    let mut methods = Vec::new();
    for each in &lineage {
        for member in &traits[each].def.members {
            let body = match (&member.body, each == trait_ref) {
                (Some(body), true) => body.clone(),
                _ => stub_body(member.name_span),
            };
            let method = provided_method(member, body, each, members.clone());
            methods.push(crate::mir::subst_method(&method, &assoc));
        }
    }
    let mut defs = archetypes.assoc_defs(span)?;
    defs.push(TypeDef {
        exported: false,
        name: archetypes.base.clone(),
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
        param_bounds: Vec::new(),
        bounds: Vec::new(),
        assoc_bounds: Vec::new(),
        mixed: None,
        assoc: archetypes.entries("", span),
        checking: Vec::new(),
    });
    Ok(defs)
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
        type_params: Vec::new(),
        provided: None,
        requires: Vec::new(),
        generic: Vec::new(),
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
        "show" => ExprKind::Call {
            callee: "__show".into(),
            args: vec![Arg::Pos(name("self"))],
            type_args: Vec::new(),
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
            type_args: Vec::new(),
        },
        _ => ExprKind::Call {
            callee: "__compare".into(),
            args: vec![Arg::Pos(name("self")), Arg::Pos(name("other"))],
            type_args: Vec::new(),
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
        type_params: Vec::new(),
        provided: None,
        requires: Vec::new(),
        generic: Vec::new(),
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
            if let Err(error) = resolve_generics(function, &module.id, scope, &mut archetypes, &HashMap::new()) {
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

/// A generic type's parameters' bounds, resolved in its module: every
/// instance's arguments must meet them (D228 §5).
fn type_bounds(package: &mut Package, scope: &Scope) -> Result<()> {
    let mut errors = Vec::new();
    for module in &mut package.modules {
        let mut archetypes = Vec::new();
        for item in &mut module.ast.items {
            let Item::Type(def) = item else { continue };
            // A method's own type parameters (and bare trait parameters),
            // resolved as a generic function's are; its archetypes are
            // `Param_Boxmap_U`.
            for method in &mut def.methods {
                let mut function = Function {
                    exported: method.exported,
                    name: format!("{}{}", def.name, method.name),
                    name_span: method.name_span,
                    type_params: method.type_params.clone(),
                    params: method.params.clone(),
                    return_type: method.return_type.clone(),
                    body: Block {
                        items: Vec::new(),
                        tail: None,
                        span: method.name_span,
                    },
                    generic: Vec::new(),
                };
                match resolve_generics(&mut function, &module.id, scope, &mut archetypes, &HashMap::new()) {
                    Ok(()) => {
                        method.type_params = function.type_params;
                        method.params = function.params;
                        method.generic = function.generic;
                    }
                    Err(error) => errors.push(error.with_path(module.path.clone())),
                }
            }
            let mut resolved = Vec::new();
            for bounds in &def.param_bounds {
                let mut list = Vec::new();
                for (name, span) in bounds {
                    match scope.trait_named(&module.id, name) {
                        Some(bound) => list.push(bound),
                        None => errors.push(
                            Error::at(format!("unknown trait `{name}`"), *span).with_path(module.path.clone()),
                        ),
                    }
                }
                resolved.push(list);
            }
            def.bounds = resolved;
        }
        module.ast.items.extend(archetypes.into_iter().map(Item::Type));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::multi(errors))
    }
}

/// A generic product's checking instances (D228 §5): archetypes for its type
/// parameters with the type's bounds, for its own methods and `from`
/// clauses (`Param_Box_T`), and with a conformance's bounds added, for the
/// members of a conformance that holds per instance (`Param_Box_when1_T`).
/// The MIR makes each instance and lowers those bodies there, so each is
/// checked once, against the bounds alone. A trait's provided members are
/// checked at the trait.
fn checking_instances(package: &mut Package, scope: &Scope) -> Result<()> {
    let mut errors = Vec::new();
    for module in &mut package.modules {
        let mut archetypes = Vec::new();
        for item in &mut module.ast.items {
            let Item::Type(def) = item else { continue };
            if def.type_params.is_empty() || def.union_members.is_some() {
                continue;
            }
            let written: Vec<&Method> = def.methods.iter().filter(|method| method.provided.is_none()).collect();
            let mut sets: Vec<Vec<Vec<TraitRef>>> = Vec::new();
            // A type whose parameters have associated types gets the base
            // set too: its instances read `S.Item` from it (D237).
            let has_assoc = def.bounds.iter().flatten().any(|bound| {
                scope.traits.get(bound).is_some_and(|entry| !entry.def.assoc.is_empty())
                    || lineage(bound, scope.traits).iter().any(|each| !scope.traits[each].def.assoc.is_empty())
            });
            if !def.conversions.is_empty() || has_assoc || written.iter().any(|method| method.requires.iter().all(Vec::is_empty)) {
                sets.push(Vec::new());
            }
            for method in &written {
                if !method.requires.iter().all(Vec::is_empty) && !sets.contains(&method.requires) {
                    sets.push(method.requires.clone());
                }
            }
            for (index, requires) in sets.into_iter().enumerate() {
                let name = if requires.is_empty() {
                    def.name.clone()
                } else {
                    format!("{}_when{index}", def.name)
                };
                let resolved: HashMap<String, Vec<TraitRef>> = def
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(position, param)| {
                        let mut bounds = def.bounds.get(position).cloned().unwrap_or_default();
                        for bound in requires.get(position).into_iter().flatten() {
                            if !bounds.contains(bound) {
                                bounds.push(bound.clone());
                            }
                        }
                        (param.clone(), bounds)
                    })
                    .collect();
                let mut function = Function {
                    exported: false,
                    name,
                    name_span: def.name_span,
                    type_params: def
                        .type_params
                        .iter()
                        .map(|param| TypeParamBound {
                            name: param.clone(),
                            span: def.name_span,
                            traits: Vec::new(),
                            same: None,
                        })
                        .chain(def.assoc_bounds.iter().cloned())
                        .collect(),
                    params: Vec::new(),
                    return_type: None,
                    body: Block {
                        items: Vec::new(),
                        tail: None,
                        span: def.name_span,
                    },
                    generic: Vec::new(),
                };
                match resolve_generics(&mut function, &module.id, scope, &mut archetypes, &resolved) {
                    Ok(()) => def.checking.push(Checking {
                        archetypes: function
                            .generic
                            .iter()
                            .filter(|param| param.derived.is_none())
                            .map(|param| param.archetype.clone())
                            .collect(),
                        requires,
                        derived: function
                            .generic
                            .iter()
                            .filter_map(|param| {
                                let (owner, assoc) = param.derived.clone()?;
                                Some(DerivedParam {
                                    name: param.name.clone(),
                                    archetype: param.archetype.clone(),
                                    owner,
                                    assoc,
                                    bounds: param.bounds.clone(),
                                    fixed: param.fixed.clone(),
                                })
                            })
                            .collect(),
                    }),
                    Err(error) => errors.push(error.with_path(module.path.clone())),
                }
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
    resolved: &HashMap<String, Vec<TraitRef>>,
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
                same: None,
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
    // `where S.Item: Equatable` (D228 §7): bounds on associated types, by the
    // parameter and the path under it.
    let mut extra: HashMap<String, HashMap<String, Vec<TraitRef>>> = HashMap::new();
    // `where S.Item == Int64` (D231): the type one is, by the parameter and
    // the path under it, in MIR names.
    let mut fixed: HashMap<String, HashMap<String, (TypeExpr, Span)>> = HashMap::new();
    let plain: Vec<String> = function
        .type_params
        .iter()
        .filter(|param| !param.name.contains('.'))
        .map(|param| param.name.clone())
        .collect();
    for param in function.type_params.iter() {
        let Some(same) = &param.same else { continue };
        if !param.name.contains('.') {
            return Err(Error::at(
                format!("`where {} == …` fixes a type parameter: write the type instead", param.name),
                param.span,
            ));
        }
        if same.name.contains('.') {
            return Err(Error::at(
                format!("`{}` is fixed as another associated type: name a type parameter for both (`where A.Item == T, B.Item == T`)", param.name),
                same.span,
            ));
        }
        let (owner, path) = param.name.split_once('.').unwrap();
        let written = scope.globalize(same, module, &(module.to_string(), String::new()));
        fixed.entry(owner.to_string()).or_default().insert(path.to_string(), (written, param.span));
    }
    for param in function.type_params.iter().filter(|param| param.name.contains('.')) {
        let (owner, path) = param.name.split_once('.').unwrap();
        let list = extra.entry(owner.to_string()).or_default().entry(path.to_string()).or_default();
        for (name, span) in &param.traits {
            let Some(bound) = scope.trait_named(module, name) else {
                return Err(Error::at(format!("unknown trait `{name}`"), *span));
            };
            list.push(bound);
        }
    }
    // In the archetypes a fixed type is that type, a type parameter by its
    // archetype.
    let archetype_of = |name: &str| format!("Param_{}_{}", function.name.replace("__", ""), name);
    let in_archetypes: HashMap<String, TypeExpr> = plain
        .iter()
        .map(|name| {
            (
                name.clone(),
                TypeExpr {
                    name: format!("{RESOLVED_TYPE}{}", scope.mangle(module, &archetype_of(name))),
                    ..void_type(function.name_span)
                },
            )
        })
        .collect();
    let mut seen = HashSet::new();
    for param in function.type_params.iter().filter(|param| !param.name.contains('.')) {
        if !seen.insert(param.name.as_str()) {
            return Err(Error::at(format!("type parameter `{}` is named twice", param.name), param.span));
        }
        if scope.type_named(module, &param.name).is_some() {
            return Err(Error::at(
                format!("type parameter `{}` has the name of a type", param.name),
                param.span,
            ));
        }
        // Bounds resolved where they were written, when that is elsewhere.
        let mut bounds = resolved.get(&param.name).cloned().unwrap_or_default();
        for (name, span) in param.traits.iter().filter(|_| !resolved.contains_key(&param.name)) {
            let Some(bound) = scope.trait_named(module, name) else {
                return Err(Error::at(format!("unknown trait `{name}`"), *span));
            };
            bounds.push(bound);
        }
        // No `__` in a type's name: the C name of its methods splits there.
        let archetype = format!("Param_{}_{}", function.name.replace("__", ""), param.name);
        let nodes = where_nodes(&param.name, &bounds, extra.remove(&param.name).unwrap_or_default(), scope, param.span)?;
        // Each fixed one by the path it is fixed at (`Item` is `Cursor.Item`),
        // a leaf: what is under it would come from the type it is fixed as.
        let mut fixed_here: HashMap<String, (TypeExpr, TypeExpr, String)> = HashMap::new();
        for (as_written, (written, span)) in fixed.remove(&param.name).unwrap_or_default() {
            let path = unalias(&as_written, &nodes);
            if nodes.iter().any(|node| node.path.starts_with(&format!("{path}."))) {
                return Err(Error::at(
                    format!("`{}.{path}` has associated types of its own: fix one of those instead", param.name),
                    span,
                ));
            }
            let archetypal = crate::mir::subst_type_expr(&written, &in_archetypes);
            fixed_here.insert(path, (written, archetypal, as_written));
        }
        let types = Archetypes {
            base: archetype.clone(),
            module: module.to_string(),
            nodes,
            scope,
            fixed: fixed_here.iter().map(|(path, (_, archetypal, _))| (path.clone(), archetypal.clone())).collect(),
        };
        archetypes.push(param_archetype(
            &archetype,
            &bounds,
            scope.traits,
            param.span,
            &types.subst("", param.span),
            types.entries("", param.span),
        )?);
        archetypes.extend(types.assoc_defs(param.span)?);
        function.generic.push(GenericParam {
            name: param.name.clone(),
            bounds,
            archetype,
            derived: None,
            fixed: None,
        });
        // `T.Item`: a parameter read off `T`'s argument, after its owner's;
        // one a trait fixes as another (`S.Item` as `S.Cursor.Item`) is
        // read where it is fixed, after that one.
        for node in &types.nodes {
            let path = unalias(&node.path, &types.nodes);
            let (owner, assoc) = match path.rsplit_once('.') {
                Some((owner, assoc)) => (format!("{}.{owner}", param.name), assoc.to_string()),
                None => (param.name.clone(), path.clone()),
            };
            function.generic.push(GenericParam {
                name: format!("{}.{}", param.name, node.path),
                bounds: types.bounds(&node.path),
                archetype: types.name(&node.path),
                derived: Some((owner, assoc)),
                // Checked where it is written (`S.Item`), as the call sees it.
                fixed: fixed_here
                    .get(&path)
                    .filter(|(_, _, as_written)| *as_written == node.path)
                    .map(|(written, _, _)| written.clone()),
            });
        }
    }
    if let Some((owner, _)) = extra.into_iter().next() {
        return Err(Error::at(format!("`{owner}` is not a type parameter of `{}`", function.name), function.name_span));
    }
    Ok(())
}

/// A type parameter's associated types, with a `where` clause's bounds on
/// them (by path, through what the traits fix them as): each path must name
/// one.
fn where_nodes(
    param: &str,
    bounds: &[TraitRef],
    mut extra: HashMap<String, Vec<TraitRef>>,
    scope: &Scope,
    span: Span,
) -> Result<Vec<AssocNode>> {
    let mut nodes = Vec::new();
    // A bound on a fixed type is on the one it is fixed as, whose own
    // associated types the bound may add; settle until nothing moves.
    for _ in 0..=ASSOC_DEPTH {
        nodes.clear();
        assoc_tree(bounds, "", &extra, scope.traits, span, &mut nodes)?;
        let mut moved: HashMap<String, Vec<TraitRef>> = HashMap::new();
        for (path, list) in &extra {
            moved.entry(unalias(path, &nodes)).or_default().extend(list.iter().cloned());
        }
        if moved.keys().all(|path| extra.contains_key(path)) && moved.len() == extra.len() {
            break;
        }
        extra = moved;
    }
    for path in extra.keys() {
        if !nodes.iter().any(|node| node.path == *path && node.alias.is_none()) {
            return Err(Error::at(format!("`{param}` has no associated type `{path}`"), span));
        }
    }
    Ok(nodes)
}

/// A type parameter's archetype: no fields, a stub for each member of its
/// bounds' lineages, conforming to all of them.
fn param_archetype(
    name: &str,
    bounds: &[TraitRef],
    traits: &HashMap<TraitRef, TraitEntry>,
    span: Span,
    assoc_types: &HashMap<String, TypeExpr>,
    assoc: Vec<(String, TypeExpr, Vec<TraitRef>)>,
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
            let method = provided_method(member, stub_body(span), each, members.clone());
            methods.push(crate::mir::subst_method(&method, assoc_types));
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
        param_bounds: Vec::new(),
        bounds: Vec::new(),
        assoc_bounds: Vec::new(),
        mixed: None,
        assoc,
        checking: Vec::new(),
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
fn canonical(
    ty: &TypeExpr,
    module: &str,
    target: &(String, String),
    scope: &Scope,
    assoc: &HashMap<String, String>,
) -> String {
    render(ty, &|name: &str| {
        if let Some(found) = assoc.get(name) {
            found.clone()
        } else if name == "Self" {
            format!("{}::{}", target.0, target.1)
        } else if let Some((from, original)) = name
            .strip_prefix(RESOLVED_TYPE)
            .and_then(|mir| scope.unmangle(mir))
            .or_else(|| scope.type_named(module, name))
            .or_else(|| scope.unmangle(name))
        {
            // A name `globalize` wrote is the MIR's.
            format!("{from}::{original}")
        } else {
            name.to_string()
        }
    })
}

/// A trait member's signature as the conforming type sees it.
fn signature(member: &TraitMember, target: &str, assoc: &HashMap<String, String>) -> String {
    let name = |name: &str| {
        if let Some(shown) = assoc.get(name) {
            shown.clone()
        } else if name == "Self" {
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
