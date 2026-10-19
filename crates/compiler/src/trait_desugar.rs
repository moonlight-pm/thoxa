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
//! Not yet (later slices of the freeze): provided members, `self`, getters and
//! setters, generic and built-in types as targets, members written outside the
//! type's module.

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
        self.import(module, name)
            .filter(|imported| self.types.contains(imported))
    }
}

pub fn desugar_package(package: &mut Package) -> Result<()> {
    let has_traits = package.modules.iter().any(|module| {
        module
            .ast
            .items
            .iter()
            .any(|item| matches!(item, Item::Trait(_) | Item::Conform(_)))
    });
    if !has_traits {
        return Ok(());
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

    let scope = Scope {
        imports: &imports,
        types,
        traits: &traits,
    };
    // The type's own member names, before any conformance adds to them.
    let mut own_members: HashMap<(String, String), HashSet<String>> = HashMap::new();
    let mut generic: HashSet<(String, String)> = HashSet::new();
    let mut unions: HashSet<(String, String)> = HashSet::new();
    for module in &package.modules {
        for item in &module.ast.items {
            let Item::Type(def) = item else { continue };
            let key = (module.id.clone(), def.name.clone());
            let names = def
                .methods
                .iter()
                .map(|method| method.name.clone())
                .chain(def.params.iter().map(|param| param.name.clone()))
                .chain(def.fields.iter().map(|field| field.name.clone()))
                .collect();
            own_members.insert(key.clone(), names);
            if def.union_members.is_some() {
                unions.insert(key.clone());
            }
            if !def.type_params.is_empty() {
                generic.insert(key);
            }
        }
    }

    // (type) → trait → where it was declared; (type) → member → its trait.
    let mut conformed: HashMap<(String, String), HashMap<TraitRef, Span>> = HashMap::new();
    let mut trait_members: HashMap<(String, String), HashMap<String, TraitRef>> = HashMap::new();
    let mut additions: HashMap<(String, String), Vec<Method>> = HashMap::new();
    let mut declared_at: Vec<((String, String), TraitRef, Span, PathBuf)> = Vec::new();

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
            Ok((target, refs, methods)) => {
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

    // A trait's parents are conformed too.
    for (target, trait_ref, span, path) in &declared_at {
        for parent in &traits[trait_ref].parents {
            if !conformed[target].contains_key(parent) {
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

    for module in &mut package.modules {
        for item in &mut module.ast.items {
            let Item::Type(def) = item else { continue };
            let key = (module.id.clone(), def.name.clone());
            if let Some(methods) = additions.remove(&key) {
                def.methods.extend(methods);
            }
            if let Some(refs) = conformed.get(&key) {
                let mut refs: Vec<TraitRef> = refs.keys().cloned().collect();
                refs.sort_by(|a, b| (&a.module, &a.name).cmp(&(&b.module, &b.name)));
                def.traits = refs;
            }
        }
    }
    Ok(())
}

/// What slice 1 accepts in a trait: required methods with distinct names.
fn check_trait(def: &TraitDef) -> Result<()> {
    let mut seen = HashSet::new();
    for member in &def.members {
        if !seen.insert(member.name.as_str()) {
            return Err(Error::at(
                format!("`{}` is declared twice in `{}`", member.name, def.name),
                member.name_span,
            ));
        }
        if let Some(body) = &member.body {
            return Err(Error::at(
                "a trait's members with bodies are not supported yet; declare the signature only",
                body.span,
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

type Checked = ((String, String), Vec<TraitRef>, Vec<Method>);

fn check_conformance(
    item: &Pending,
    scope: &Scope,
    own_members: &HashMap<(String, String), HashSet<String>>,
    generic: &HashSet<(String, String)>,
    unions: &HashSet<(String, String)>,
    conformed: &mut HashMap<(String, String), HashMap<TraitRef, Span>>,
    trait_members: &mut HashMap<(String, String), HashMap<String, TraitRef>>,
) -> Result<Checked> {
    let conformance = &item.conformance;
    let ty = &conformance.ty;
    if ty.lit.is_some()
        || ty.union_members.is_some()
        || ty.fn_type.is_some()
        || ty.where_clause.is_some()
        || ty.mark.is_some()
    {
        return Err(Error::at("`conform` names a declared type", ty.span));
    }
    if BUILT_IN_TYPES.contains(&ty.name.as_str()) {
        return Err(Error::at(
            format!("conforming the built-in type `{}` is not supported yet", ty.name),
            ty.span,
        ));
    }
    let Some(target) = scope.type_named(&item.module, &ty.name) else {
        let message = if scope.trait_named(&item.module, &ty.name).is_some() {
            format!("`{}` is a trait; `conform` names a type first: `conform Type to {}`", ty.name, ty.name)
        } else {
            format!("unknown type `{}`", ty.name)
        };
        return Err(Error::at(message, ty.span));
    };
    if !ty.args.is_empty() || generic.contains(&target) {
        return Err(Error::at(
            format!("a generic type cannot conform yet (`{}`)", ty.name),
            ty.span,
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
    let empty = HashSet::new();
    let own = own_members.get(&target).unwrap_or(&empty);
    let mut methods = Vec::new();
    for (trait_ref, span) in &refs {
        let entry = &scope.traits[trait_ref];
        for member in &entry.def.members {
            if own.contains(&member.name) {
                return Err(Error::at(
                    format!(
                        "`{}` already has a member named `{}`, so it cannot take `{}`'s",
                        target.1, member.name, trait_ref.name
                    ),
                    *span,
                ));
            }
            let taken = trait_members.entry(target.clone()).or_default();
            if let Some(other) = taken.get(&member.name) {
                return Err(Error::at(
                    format!(
                        "`{}` is a member of both `{}` and `{}`, and `{}` conforms to both",
                        member.name, other.name, trait_ref.name, target.1
                    ),
                    *span,
                ));
            }
            taken.insert(member.name.clone(), trait_ref.clone());
            let Some(method) = implemented.get(member.name.as_str()) else {
                return Err(Error::at(
                    format!(
                        "`{}` does not implement `{}` from `{}`",
                        target.1,
                        signature(member, &target.1),
                        trait_ref.name
                    ),
                    conformance.span,
                ));
            };
            methods.push(implementation(method, member, trait_ref, item, &target, scope)?);
        }
    }
    let done = conformed.entry(target.clone()).or_default();
    for (trait_ref, span) in &refs {
        done.insert(trait_ref.clone(), *span);
    }
    Ok((target, refs.into_iter().map(|(trait_ref, _)| trait_ref).collect(), methods))
}

/// The conformance's method, checked against the trait member it implements:
/// public, with `Self` replaced and the trait's return type when it writes none.
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
    if let Some(written) = &method.return_type {
        if in_conformance(written) != in_trait(&declared_return) {
            return Err(mismatch());
        }
    }
    let mut method = method.clone();
    method.exported = true;
    for param in &mut method.params {
        replace_self(&mut param.ty, &target.1);
    }
    let mut return_type = method.return_type.take().unwrap_or(declared_return);
    replace_self(&mut return_type, &target.1);
    method.return_type = Some(return_type);
    Ok(method)
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

/// `Self` written in a conformance's signature is the conforming type. A
/// conformance's members are in the type's module, so its name reaches it.
fn replace_self(ty: &mut TypeExpr, target: &str) {
    if ty.name == "Self" && ty.lit.is_none() {
        ty.name = target.to_string();
    }
    for arg in &mut ty.args {
        if let TypeArg::Type(inner) = arg {
            replace_self(inner, target);
        }
    }
    for member in ty.union_members.iter_mut().flatten() {
        replace_self(member, target);
    }
    if let Some(fn_type) = &mut ty.fn_type {
        for (_, param) in &mut fn_type.params {
            replace_self(param, target);
        }
        replace_self(&mut fn_type.ret, target);
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
