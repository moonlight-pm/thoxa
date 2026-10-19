//! D228 §5: generic functions.
//!
//! `fn largest<T: Ordered>(items: Array<T>) -> May<T>` is a template, not a
//! function. A call infers `T` from its arguments (and, failing that, from
//! the type its result is expected to have), checks the bounds, and calls
//! the instance for those type arguments (`largest__Int64`), made on first
//! use and lowered after the bodies that asked for it. Every template is also
//! instantiated once with its archetypes (`Param_largest_T`, which have only
//! the bounds' members): that instance is never called, but lowering it
//! checks the body against the bounds alone, before any real instance.
//!
//! An instance's signature may name a generic type's instance nothing else
//! named (`May<Point>`); it is made then, like the ones named in the source.

use std::collections::HashMap;
use std::rc::Rc;

use super::{
    bind_param, define_method_scope, display_ty, infer_block_ty, mono_type_name, monomorphize_pending,
    probe_body_return, resolve_ty, subst_expr, subst_method, subst_type_expr, tail_block,
    ty_conforms, ty_mono_suffix, FnSig, LowerCtx, Ty,
};
use crate::ast::{Arg, Expr, ExprKind, Function, GenericParam, Method, Param, TypeArg, TypeExpr};
use crate::error::{Error, Result};
use crate::span::Span;

/// A generic function's template and where it is written.
#[derive(Clone)]
pub(super) struct GenericFn {
    pub(super) function: Function,
    pub(super) module: String,
    pub(super) is_entry: bool,
}

/// An instance to lower: its template, its name, and its type arguments.
#[derive(Clone)]
pub(super) struct InstanceJob {
    pub(super) template: String,
    pub(super) instance: String,
    pub(super) subst: HashMap<String, Ty>,
    /// The checking instance, made with the template's archetypes.
    pub(super) archetypal: bool,
}

/// The instance a call to `template` (as `shown`) needs: its name, with its
/// signature in `ctx.fns`.
pub(super) fn instantiate_call(
    ctx: &mut LowerCtx,
    template_name: &str,
    shown: &str,
    args: &[Arg],
    span: Span,
    expect: Option<&Ty>,
    explicit: &[Ty],
) -> Result<String> {
    let template = ctx.synthetics.borrow().generic_fns[template_name].clone();
    let function = &template.function;
    // `S.Item` is not inferred: it is read off `S`'s argument.
    let names: Vec<String> = function
        .generic
        .iter()
        .filter(|param| param.derived.is_none())
        .map(|param| param.name.clone())
        .collect();
    // Written ones are the function's own: an `extend` method's are those
    // its receiver does not fix (`map<U>`'s `U`, not `Array<T>`'s `T`).
    let own: Vec<String> = match function.params.first() {
        Some(receiver) if function.name.starts_with("__extend_") && receiver.name == "self" => {
            names.iter().filter(|name| !mentions(&receiver.ty, std::slice::from_ref(name))).cloned().collect()
        }
        _ => names.clone(),
    };
    let mut bindings = written_bindings(&own, explicit, shown, span)?;
    for (param, arg) in function.params.iter().zip(match_args(&function.params, args)) {
        let Some(arg) = arg else { continue };
        if !mentions(&param.ty, &names) {
            continue;
        }
        let ty = infer_block_ty(ctx, &tail_block(probe(arg)))?;
        bind(&param.ty, &ty, &names, &mut bindings, ctx, arg.span, shown)?;
    }
    if let (Some(ret), Some(expect)) = (&function.return_type, expect) {
        if names.iter().any(|name| !bindings.contains_key(name)) && mentions(ret, &names) {
            let _ = bind(ret, expect, &names, &mut bindings, ctx, span, shown);
        }
    }
    let help = format!(
        "write them out, `{shown}<{}>(…)`, or annotate what the call's result is assigned to",
        own.join(", ")
    );
    complete_bindings(ctx, &function.generic, &mut bindings, shown, span, &help)?;
    let instance = instance_name(template_name, function, &bindings);
    let existing = ctx.synthetics.borrow().fn_instances.get(&instance).cloned();
    let sig = match existing {
        Some(sig) => sig,
        None => {
            let sig = instance_sig(ctx, &template, &instance, &bindings)?;
            let mut synthetics = ctx.synthetics.borrow_mut();
            synthetics.fn_instances.insert(instance.clone(), sig.clone());
            synthetics.instance_jobs.push(InstanceJob {
                template: template_name.to_string(),
                instance: instance.clone(),
                subst: bindings,
                archetypal: false,
            });
            sig
        }
    };
    ctx.fns.insert(instance.clone(), sig);
    Ok(instance)
}

/// An argument as its type is inferred: `mut items` has the type of `items`.
fn probe(arg: &Expr) -> Expr {
    match &arg.kind {
        ExprKind::MutArg(binding) => Expr {
            kind: ExprKind::Name(binding.clone()),
            span: arg.span,
        },
        _ => arg.clone(),
    }
}

/// The type arguments a call writes out (D230), bound to `own` in order:
/// all of them, or none.
fn written_bindings(own: &[String], explicit: &[Ty], shown: &str, span: Span) -> Result<HashMap<String, Ty>> {
    if explicit.is_empty() {
        return Ok(HashMap::new());
    }
    if explicit.len() != own.len() {
        return Err(Error::at(
            format!(
                "`{shown}` takes {} type argument{}, and the call writes {}",
                own.len(),
                if own.len() == 1 { "" } else { "s" },
                explicit.len()
            ),
            span,
        ));
    }
    Ok(own.iter().cloned().zip(explicit.iter().cloned()).collect())
}

/// Bind a template's parameters that are not inferred (`S.Item`, read off
/// `S`'s argument), and check every one is bound and meets its bounds.
fn complete_bindings(
    ctx: &mut LowerCtx,
    generic: &[GenericParam],
    bindings: &mut HashMap<String, Ty>,
    shown: &str,
    span: Span,
    help: &str,
) -> Result<()> {
    ctx.take_new_instances();
    // Associated types, read off their owners' arguments (an owner not
    // bound is reported below); one a `where` fixes must be that type, or
    // binds the type parameter it names (D231).
    for param in generic {
        let Some((owner, assoc)) = &param.derived else { continue };
        let Some(owner_ty) = bindings.get(owner).cloned() else { continue };
        let found = super::assoc_type(ctx, &owner_ty, assoc, span)?;
        let Some(ty) = found else {
            return Err(Error::at(
                format!("`{shown}` needs `{owner}.{assoc}`, and {} names no `{assoc}`", display_ty(&owner_ty)),
                span,
            ));
        };
        bindings.insert(param.name.clone(), ty.clone());
        let Some(fixed) = &param.fixed else { continue };
        let names_param = fixed.args.is_empty()
            && generic.iter().any(|each| each.derived.is_none() && each.name == fixed.name);
        let want = match bindings.get(&fixed.name) {
            None if names_param => {
                bindings.insert(fixed.name.clone(), ty);
                continue;
            }
            Some(bound) if names_param => bound.clone(),
            _ => {
                let written = super::subst_type_expr(fixed, &*bindings);
                let name_map = ctx.name_to_type.clone();
                ensure_instances(ctx, &written, &name_map)?;
                resolve_ty(&written, &ctx.types, &name_map)?
            }
        };
        if want != ty {
            // Named by the argument the call passes, not a cursor under it.
            let root = param.name.split('.').next().and_then(|root| bindings.get(root)).unwrap_or(&owner_ty);
            return Err(Error::at(
                format!(
                    "`{shown}` needs `{}` to be {}, and {}'s is {}",
                    param.name,
                    display_ty(&want),
                    display_ty(root),
                    display_ty(&ty)
                ),
                span,
            ));
        }
    }
    for param in generic {
        let Some(ty) = bindings.get(&param.name) else {
            return Err(Error::at(
                format!("cannot infer `{}` for `{shown}` from its arguments", param.name),
                span,
            )
            .with_help(help));
        };
        let conditional = ctx.synthetics.borrow().conditional.clone();
        for bound in &param.bounds {
            if !ty_conforms(&ctx.types, &conditional, ty, bound) {
                return Err(Error::at(
                    format!(
                        "`{shown}` needs `{}: {}`, and {} is not `{}`",
                        param.name,
                        bound.name,
                        display_ty(ty),
                        bound.name
                    ),
                    span,
                ));
            }
        }
    }
    Ok(())
}

/// A method made from a template (D228 §5), whose body is still to lower.
#[derive(Clone)]
pub(super) struct MethodJob {
    pub(super) type_name: String,
    pub(super) method: Method,
    pub(super) def_mod: String,
    /// Its template: its type and name. A template whose checking method
    /// fails makes no others.
    pub(super) template: (String, String),
    /// The checking method, made with the template's archetypes.
    pub(super) archetypal: bool,
}

/// The method a call to `type_name`'s template `method` needs (D228 §5): its
/// type arguments inferred from the call's arguments, made on first use, its
/// signature in the type's methods.
pub(super) fn instantiate_method(
    ctx: &mut LowerCtx,
    type_name: &str,
    method: &str,
    args: &[Arg],
    span: Span,
    explicit: &[Ty],
) -> Result<String> {
    let template = ctx.types[type_name].templates[method].clone();
    let shown = format!("{}.{method}", display_ty(&Ty::User(type_name.to_string())));
    let generic = &template.method.generic;
    let names: Vec<String> = generic
        .iter()
        .filter(|param| param.derived.is_none())
        .map(|param| param.name.clone())
        .collect();
    let params = &template.method.params;
    let mut bindings = written_bindings(&names, explicit, &shown, span)?;
    for (param, arg) in params.iter().zip(match_args(params, args)) {
        let Some(arg) = arg else { continue };
        if !mentions(&param.ty, &names) {
            continue;
        }
        let ty = infer_block_ty(ctx, &tail_block(probe(arg)))?;
        bind(&param.ty, &ty, &names, &mut bindings, ctx, arg.span, &shown)?;
    }
    let help = format!("write them out, `.{method}<{}>(…)`: a method's come from its arguments", names.join(", "));
    complete_bindings(ctx, generic, &mut bindings, &shown, span, &help)?;
    method_instance(ctx, type_name, &template, bindings, false)
}

/// Each template of `type_name` checked once, as its method for its
/// archetypes, which have only their bounds' members (D228 §5).
pub(super) fn check_templates(ctx: &mut LowerCtx, type_name: &str) -> Result<()> {
    let templates: Vec<super::MethodTemplate> = ctx
        .types
        .get(type_name)
        .map(|info| info.templates.values().cloned().collect())
        .unwrap_or_default();
    for template in templates {
        let name_map = ctx
            .package_maps
            .get(&template.def_mod)
            .map(|maps| maps.name_to_type.clone())
            .unwrap_or_default();
        let bindings: HashMap<String, Ty> = template
            .method
            .generic
            .iter()
            .map(|param| {
                let archetype = name_map.get(&param.archetype).cloned().unwrap_or_else(|| param.archetype.clone());
                (param.name.clone(), Ty::User(archetype))
            })
            .collect();
        method_instance(ctx, type_name, &template, bindings, true)?;
    }
    Ok(())
}

/// `template`'s method for `bindings` (`map_String`), made if it does not
/// exist yet: its signature joins the type's methods, and its body the late
/// bodies.
fn method_instance(
    ctx: &mut LowerCtx,
    type_name: &str,
    template: &super::MethodTemplate,
    bindings: HashMap<String, Ty>,
    archetypal: bool,
) -> Result<String> {
    let mut name = template.method.name.clone();
    for param in template.method.generic.iter().filter(|param| param.derived.is_none()) {
        let arg = bindings.get(&param.name).cloned().unwrap_or(Ty::Void);
        // A C method name splits at its last `__`.
        name.push('_');
        name.push_str(&ty_mono_suffix(&arg).replace("__", "_"));
    }
    if ctx.types.get(type_name).is_some_and(|info| info.methods.contains_key(&name)) {
        return Ok(name);
    }
    let made = ctx
        .synthetics
        .borrow()
        .method_instances
        .get(type_name)
        .and_then(|methods| methods.get(&name))
        .cloned();
    if let Some(sig) = made {
        ctx.types.get_mut(type_name).unwrap().methods.insert(name.clone(), sig);
        return Ok(name);
    }
    let mut method = subst_method(&template.method, &bindings);
    method.name = name.clone();
    method.generic.clear();
    method.type_params.clear();
    let maps = ctx.package_maps.get(&template.def_mod).cloned().unwrap_or_default();
    let mut name_map = maps.name_to_type.clone();
    name_map.insert("Self".into(), type_name.to_string());
    let mut params = Vec::new();
    for param in &method.params {
        ensure_instances(ctx, &param.ty, &name_map)?;
        params.push((param.name.clone(), resolve_ty(&param.ty, &ctx.types, &name_map)?));
    }
    let ret = match &method.return_type {
        Some(ret) => {
            ensure_instances(ctx, ret, &name_map)?;
            resolve_ty(ret, &ctx.types, &name_map)?
        }
        None => Ty::Void,
    };
    let mut sig = super::MethodSig {
        params,
        defaults: method.params.iter().map(|param| param.default.clone()).collect(),
        ret,
        exported: method.exported,
        def_mod: template.def_mod.clone(),
        multi_clause: false,
        mutable: method.params.iter().map(|param| param.mutable).collect(),
        positional: method.params.iter().map(|param| param.positional).collect(),
        requires: Vec::new(),
    };
    ctx.types.get_mut(type_name).unwrap().methods.insert(name.clone(), sig.clone());
    // Without `->`, its return type comes from its body's exits (D174).
    if method.return_type.is_none() {
        let mut probe = LowerCtx::new(
            ctx.fns.clone(),
            ctx.types.clone(),
            maps.name_to_type,
            maps.name_to_fn,
            maps.name_to_global,
            maps.namespaces,
            ctx.module_exports.clone(),
            ctx.package_maps.clone(),
            Ty::Void,
            template.def_mod.clone(),
            false,
            Rc::clone(&ctx.synthetics),
        );
        probe.inferring = true;
        probe.infer_self = Some(format!("{type_name}__{name}"));
        let generics = ctx.synthetics.borrow().generics.clone();
        probe.generics = generics.clone();
        define_method_scope(&mut probe, type_name, &method)?;
        let mut types = ctx.types.clone();
        let mut layouts = Vec::new();
        sig.ret = probe_body_return(probe, &method.body, method.name_span, &generics, &mut types, &mut layouts)?;
        for (each, info) in types {
            ctx.types.entry(each).or_insert(info);
        }
        ctx.types.get_mut(type_name).unwrap().methods.insert(name.clone(), sig.clone());
    }
    // A generic type's instances share their template's check.
    let owner = ctx.types[type_name]
        .instance
        .as_ref()
        .map_or_else(|| type_name.to_string(), |(base, _)| base.clone());
    let mut synthetics = ctx.synthetics.borrow_mut();
    synthetics
        .method_instances
        .entry(type_name.to_string())
        .or_default()
        .insert(name.clone(), sig);
    synthetics.method_jobs.push(MethodJob {
        type_name: type_name.to_string(),
        method,
        def_mod: template.def_mod.clone(),
        template: (owner, template.method.name.clone()),
        archetypal,
    });
    Ok(name)
}

/// `largest__Int64`: the template's name and each type argument.
pub(super) fn instance_name(template: &str, function: &Function, bindings: &HashMap<String, Ty>) -> String {
    let args: Vec<Ty> = function
        .generic
        .iter()
        .filter(|param| param.derived.is_none())
        .map(|param| bindings.get(&param.name).cloned().unwrap_or(Ty::Void))
        .collect();
    let mut name = template.to_string();
    for arg in &args {
        name.push_str("__");
        name.push_str(&ty_mono_suffix(arg));
    }
    name
}

/// An instance's signature: the template's, with its type arguments, resolved
/// in the template's module.
pub(super) fn instance_sig(
    ctx: &mut LowerCtx,
    template: &GenericFn,
    instance: &str,
    bindings: &HashMap<String, Ty>,
) -> Result<FnSig> {
    let function = &template.function;
    let name_map = ctx
        .package_maps
        .get(&template.module)
        .map(|maps| maps.name_to_type.clone())
        .unwrap_or_default();
    let mut params = Vec::new();
    let mut defaults = Vec::new();
    for param in &function.params {
        let ty = subst_type_expr(&param.ty, bindings);
        ensure_instances(ctx, &ty, &name_map)?;
        params.push((param.name.clone(), resolve_ty(&ty, &ctx.types, &name_map)?));
        defaults.push(param.default.as_ref().map(|default| subst_expr(default, bindings)));
    }
    let ret = match &function.return_type {
        Some(ret) => {
            let ty = subst_type_expr(ret, bindings);
            ensure_instances(ctx, &ty, &name_map)?;
            resolve_ty(&ty, &ctx.types, &name_map)?
        }
        // Without `->`, inferred for each instance from its body (D129).
        None => infer_instance_return(ctx, template, instance, bindings, &params)?,
    };
    Ok(FnSig {
        params,
        defaults,
        ret,
        extern_symbol: None,
        mangled: instance.to_string(),
        def_mod: template.module.clone(),
        multi_clause: false,
        mutable: function.params.iter().map(|param| param.mutable).collect(),
        positional: function.params.iter().map(|param| param.positional).collect(),
    })
}

/// An instance's return type, from its body's exits.
fn infer_instance_return(
    ctx: &mut LowerCtx,
    template: &GenericFn,
    instance: &str,
    bindings: &HashMap<String, Ty>,
    params: &[(String, Ty)],
) -> Result<Ty> {
    let maps = ctx.package_maps.get(&template.module).cloned().unwrap_or_default();
    let mut probe = LowerCtx::new(
        ctx.fns.clone(),
        ctx.types.clone(),
        maps.name_to_type,
        maps.name_to_fn,
        maps.name_to_global,
        maps.namespaces,
        ctx.module_exports.clone(),
        ctx.package_maps.clone(),
        Ty::Void,
        template.module.clone(),
        false,
        Rc::clone(&ctx.synthetics),
    );
    probe.inferring = true;
    probe.infer_self = Some(instance.to_string());
    let generics = ctx.synthetics.borrow().generics.clone();
    probe.generics = generics.clone();
    for (param, (_, ty)) in template.function.params.iter().zip(params) {
        bind_param(&mut probe, &param.name, ty.clone(), param.mutable);
    }
    let body = super::subst_block(&template.function.body, bindings);
    let mut types = ctx.types.clone();
    let mut layouts = Vec::new();
    let ret = probe_body_return(probe, &body, template.function.name_span, &generics, &mut types, &mut layouts)?;
    for (name, info) in types {
        ctx.types.entry(name).or_insert(info);
    }
    Ok(ret)
}

/// Make the instances of generic types `te` names that do not exist yet:
/// their types and layouts join the package's synthetic types, and their
/// methods are lowered with the other late bodies.
pub(super) fn ensure_instances(
    ctx: &mut LowerCtx,
    te: &TypeExpr,
    name_map: &HashMap<String, String>,
) -> Result<()> {
    for arg in &te.args {
        if let TypeArg::Type(inner) = arg {
            ensure_instances(ctx, inner, name_map)?;
        }
    }
    if te.args.is_empty() {
        return Ok(());
    }
    let base = name_map.get(&te.name).cloned().unwrap_or_else(|| te.name.clone());
    let generics = ctx.synthetics.borrow().generics.clone();
    if !generics.contains_key(&base) {
        return Ok(());
    }
    let mut args = Vec::new();
    for arg in &te.args {
        match arg {
            TypeArg::Type(inner) => args.push(resolve_ty(inner, &ctx.types, name_map)?),
            TypeArg::Int { .. } => return Ok(()),
        }
    }
    make_instance(ctx, &base, args, name_map, te.span)?;
    Ok(())
}

/// The generic type `base`'s instance for `args`, made now if it does not
/// exist: it and any instances it needs join the package's synthetic types,
/// and their methods are lowered with the other late bodies.
pub(super) fn make_instance(
    ctx: &mut LowerCtx,
    base: &str,
    args: Vec<Ty>,
    name_map: &HashMap<String, String>,
    span: Span,
) -> Result<String> {
    let mono = mono_type_name(base, &args);
    if ctx.types.contains_key(&mono) {
        return Ok(mono);
    }
    let generics = ctx.synthetics.borrow().generics.clone();
    let conditional = ctx.synthetics.borrow().conditional.clone();
    super::instance_meets_bounds(&ctx.types, &conditional, &generics, base, &args)
        .map_err(|error| Error::at(error.message, span))?;
    let mut types = ctx.types.clone();
    let mut layouts = Vec::new();
    let mut jobs = Vec::new();
    monomorphize_pending(
        vec![(base.to_string(), args, name_map.clone(), span)],
        &mut types,
        &mut layouts,
        &generics,
        &mut jobs,
    )?;
    // Methods without `->` infer their return type per instance (D211), as
    // the instances made before lowering do; twice, so a sibling's is known.
    let probed: Vec<&super::MonoMethodJob> = jobs
        .iter()
        .filter(|job| !job.conversion && job.method.return_type.is_none() && super::job_applies(&types, &ctx.synthetics, job))
        .collect();
    for _ in 0..2 {
        for job in &probed {
            let method = subst_method(&job.method, &job.subst);
            let maps = ctx.package_maps.get(&job.def_mod).cloned().unwrap_or_default();
            let mut probe = LowerCtx::new(
                ctx.fns.clone(),
                types.clone(),
                maps.name_to_type,
                maps.name_to_fn,
                maps.name_to_global,
                maps.namespaces,
                ctx.module_exports.clone(),
                ctx.package_maps.clone(),
                Ty::Void,
                job.def_mod.clone(),
                false,
                Rc::clone(&ctx.synthetics),
            );
            probe.inferring = true;
            probe.infer_self = Some(format!("{}__{}", job.mono_type, method.name));
            probe.generics = generics.clone();
            define_method_scope(&mut probe, &job.mono_type, &method)?;
            let ret = probe_body_return(probe, &method.body, method.name_span, &generics, &mut types, &mut layouts)?;
            if let Some(sig) = types
                .get_mut(&job.mono_type)
                .and_then(|info| info.methods.get_mut(&method.name))
            {
                sig.ret = ret;
            }
        }
    }
    let mut synthetics = ctx.synthetics.borrow_mut();
    for mut layout in layouts {
        (layout.own_hash, layout.own_equals) = super::own_key_ops(&types, &synthetics.conditional, &layout.name);
        if let Some(info) = types.get(&layout.name) {
            ctx.types.insert(layout.name.clone(), info.clone());
            synthetics
                .entries
                .insert(layout.name.clone(), (info.clone(), layout));
        }
    }
    // Their method templates are checked on their template's checking
    // instance, not again for each.
    synthetics.late_mono_jobs.extend(jobs);
    Ok(mono)
}

/// A generic product's constructor called without an expected type to pick
/// its instance: the type arguments come from its field arguments (D228 §5),
/// `Shelf(items: [1, 2])` is a `Shelf<Int64>`. `None` when `base` is not a
/// generic product.
pub(super) fn infer_constructor(
    ctx: &mut LowerCtx,
    base: &str,
    shown: &str,
    args: &[Arg],
    span: Span,
) -> Result<Option<String>> {
    let Some(gdef) = ctx.synthetics.borrow().generics.get(base).cloned() else {
        return Ok(None);
    };
    let super::GenericKind::Product { params, .. } = &gdef.kind else {
        return Ok(None);
    };
    let names = gdef.type_params.clone();
    let mut bindings: HashMap<String, Ty> = HashMap::new();
    for (param, arg) in params.iter().zip(match_args(params, args)) {
        let Some(arg) = arg else { continue };
        if !mentions(&param.ty, &names) {
            continue;
        }
        let ty = infer_block_ty(ctx, &tail_block(probe(arg)))?;
        bind(&param.ty, &ty, &names, &mut bindings, ctx, arg.span, shown)?;
    }
    let mut args_tys = Vec::new();
    for name in &names {
        let Some(ty) = bindings.get(name) else {
            return Err(Error::at(
                format!("cannot infer `{name}` for `{shown}` from its arguments"),
                span,
            )
            .with_help(format!("annotate the value: `let x: {shown}<…> = …`")));
        };
        args_tys.push(ty.clone());
    }
    let name_map = ctx
        .package_maps
        .get(&gdef.def_mod)
        .map(|maps| maps.name_to_type.clone())
        .unwrap_or_default();
    make_instance(ctx, base, args_tys, &name_map, span).map(Some)
}

/// Whether `te` names one of `params`.
fn mentions(te: &TypeExpr, params: &[String]) -> bool {
    (te.args.is_empty() && params.contains(&te.name))
        || te.args.iter().any(|arg| matches!(arg, TypeArg::Type(inner) if mentions(inner, params)))
        || te.union_members.iter().flatten().any(|member| mentions(member, params))
        || te.fn_type.as_ref().is_some_and(|fn_type| {
            fn_type.params.iter().any(|(_, param)| mentions(param, params)) || mentions(&fn_type.ret, params)
        })
}

/// Bind the type parameters `te` names by matching it against `ty`.
fn bind(
    te: &TypeExpr,
    ty: &Ty,
    params: &[String],
    bindings: &mut HashMap<String, Ty>,
    ctx: &LowerCtx,
    span: Span,
    shown: &str,
) -> Result<()> {
    if te.args.is_empty() && te.lit.is_none() && params.contains(&te.name) {
        if *ty == Ty::Never {
            return Ok(());
        }
        match bindings.get(&te.name) {
            Some(bound) if bound != ty => {
                return Err(Error::at(
                    format!(
                        "`{shown}` takes one type for `{}`, and gets {} and {}",
                        te.name,
                        display_ty(bound),
                        display_ty(ty)
                    ),
                    span,
                ))
            }
            Some(_) => {}
            None => {
                bindings.insert(te.name.clone(), ty.clone());
            }
        }
        return Ok(());
    }
    let inner = |index: usize| match te.args.get(index) {
        Some(TypeArg::Type(inner)) => Some(inner),
        _ => None,
    };
    match (te.name.as_str(), ty) {
        // `[]` says nothing about its elements.
        ("Array", Ty::Array { elem }) if **elem == Ty::Void => {}
        ("Array", Ty::Array { elem }) => {
            if let Some(element) = inner(0) {
                bind(element, elem, params, bindings, ctx, span, shown)?;
            }
        }
        ("Map", Ty::Map { key, val }) => {
            if let (Some(k), Some(v)) = (inner(0), inner(1)) {
                bind(k, key, params, bindings, ctx, span, shown)?;
                bind(v, val, params, bindings, ctx, span, shown)?;
            }
        }
        (_, Ty::User(name)) if !te.args.is_empty() => {
            if let Some((_, args)) = ctx.types.get(name).and_then(|info| info.instance.clone()) {
                for (index, arg) in args.iter().enumerate() {
                    if let Some(expr) = inner(index) {
                        bind(expr, arg, params, bindings, ctx, span, shown)?;
                    }
                }
            }
        }
        (_, Ty::Fn { params: arg_params, ret }) => {
            if let Some(fn_type) = &te.fn_type {
                for ((_, param), arg) in fn_type.params.iter().zip(arg_params) {
                    bind(param, arg, params, bindings, ctx, span, shown)?;
                }
                bind(&fn_type.ret, ret, params, bindings, ctx, span, shown)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Each parameter's argument, as a call writes them: `!` parameters by
/// position, the rest by label, or by a bare name that is the label (D200,
/// D202). Only for inference; the call itself is checked as any call is.
fn match_args<'a>(params: &[Param], args: &'a [Arg]) -> Vec<Option<&'a Expr>> {
    let mut out: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut positional = params
        .iter()
        .enumerate()
        .filter(|(_, param)| param.positional)
        .map(|(index, _)| index);
    for arg in args {
        match arg {
            Arg::Named { name, value, .. } => {
                if let Some(index) = params.iter().position(|param| &param.name == name) {
                    out[index] = Some(value);
                }
            }
            Arg::Pos(value) => {
                if let Some(index) = positional.next() {
                    out[index] = Some(value);
                } else if let ExprKind::Name(name) = &value.kind {
                    if let Some(index) = params.iter().position(|param| &param.name == name) {
                        out[index] = Some(value);
                    }
                }
            }
            Arg::Spread { .. } => {}
        }
    }
    out
}
