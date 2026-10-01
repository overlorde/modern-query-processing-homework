//! The three basic homework lowerings and the supplied pull-strategy lowerings.
//!
//! The third homework pass targets a mixed-stage Rust file containing one
//! local `pull!` invocation. Storage objects, their construction code, Rust
//! names, access expressions, output value, and the surrounding query API
//! must already be literal Rust syntax in that target. The supplied `pull`
//! pass first exposes a left-deep iterator plan; a separate semantic lowering
//! chooses the concrete lazy Rust iterator adapters and returns `syn::Expr`.

use std::collections::{BTreeMap, BTreeSet};
use std::{collections::HashMap, vec};

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

use cq_ir::{
    cq, index_requirements, iterator_pipeline, relational_plan, rust_access_plan, symbol_name,
};

use crate::pass_contracts;

pub struct map_val_struct {
    // Kept as the original Ident so raw names like `r#type` survive to output.
    pub relation: syn::Ident,
    pub source: Vec<String>,
    pub target: Vec<String>,
    pub is_rename: bool,
}

/// Basic Homework 1: lower a CQ to canonical named relational algebra.
#[contracts::contract(
    debug_requires(
        cq::contract::well_formed(source),
        "source must satisfy the CQ local contract"
    ),
    debug_ensures(
        ret.as_ref()
            .is_ok_and(relational_plan::contract::well_formed)
    ),
    debug_ensures(
        ret.as_ref()
            .is_ok_and(|target| target.program == source.program)
    ),
    debug_ensures(
        ret.as_ref()
            .is_ok_and(|target| pass_contracts::relational_plan_is_exact(source, target))
    ),
)]
pub fn compile_cq(source: &cq::Module) -> syn::Result<relational_plan::Module> {
    // HOMEWORK 1: construct the complete canonical RelationalPlan.
    let program = &source.program;

    let mut definitons = Vec::<TokenStream>::new();
    let mut next = 0usize;
    let mut current: Option<syn::Ident> = None;

    for item in &program.query.body {
        let cq::BodyItem::Positive { atom } = item;

        let mut found = None;

        for input in program.inputs.iter() {
            if symbol_name(&input.name) == symbol_name(&atom.relation) {
                found = Some(input);
                break;
            }
        }

        let Some(declaration) = found else {
            return Err(syn::Error::new_spanned(
                atom,
                format!("input relation `{}` is not declared", atom.relation),
            ));
        };

        let mut mapping = Vec::new();

        for i in 0..atom.variables.len() {
            let variable = &atom.variables[i];
            let column = &declaration.columns[i].name;

            mapping.push(quote! {#column -> #variable});
        }

        let relation = &atom.relation;
        let renamed = next_result(&mut next);
        definitons.push(quote! {#renamed = rename #relation {#(#mapping),*};});

        match current {
            Some(left) => {
                let joined = next_result(&mut next);
                definitons.push(quote! {#joined = natural_join #left with #renamed;});
                current = Some(joined);
            }
            None => {
                current = Some(renamed);
            }
        }
    }

    let joined = match current {
        Some(joined) => joined,
        None => {
            return Err(syn::Error::new_spanned(
                program,
                "the body of the query is empty",
            ));
        }
    };

    let projected = next_result(&mut next);
    let head = &program.query.head;
    let keep = &head.variables;
    definitons.push(quote! {#projected = project #joined keep {#keep};});

    syn::parse2(quote! {
        #program
        relational {
            #(#definitons)*
            output #projected as #head;
        }
    })
}

/// Basic Homework 2: add the exact equality-index overlay to a logical plan.
#[allow(unreachable_code, clippy::diverging_sub_expression)]
#[contracts::contract(
    debug_requires(
        relational_plan::contract::well_formed(source),
        "source must satisfy the RelationalPlan local contract"
    ),
    debug_ensures(
        pass_contracts::basic_left_deep_plan(source)
            -> ret.as_ref().is_ok_and(index_requirements::contract::well_formed)
    ),
    debug_ensures(
        pass_contracts::basic_left_deep_plan(source)
            -> ret.as_ref().is_ok_and(|target| pass_contracts::indexes_are_exact(source, target))
    ),
    debug_ensures(
        !pass_contracts::basic_left_deep_plan(source) -> ret.is_err()
    ),
)]

pub fn compile_relational_plan(
    source: &relational_plan::Module,
) -> syn::Result<index_requirements::Module> {
    let mut stream_map: HashMap<String, map_val_struct> = HashMap::new();
    let relational_plan = &source;

    let mut ans: Vec<TokenStream> = Vec::new();
    // Every (relation, columns) pair already emitted. Lives outside the loop so
    // two joins asking for the same index only produce one requirement.
    let mut seen: BTreeSet<(String, Vec<i32>)> = BTreeSet::new();
    for definition in &source.plan.definitions {
        match &definition.operator {
            relational_plan::Operator::Rename(rename) => {
                //eprintln!("Rename: {:?}", rename.relation.to_string());
                let mut source_vec = Vec::new();
                let mut target_vec = Vec::new();
                for mapping in &rename.mapping {
                    // symbol_name strips the raw `r#` prefix, so `r#x` and `x`
                    // compare as the same variable later on.
                    source_vec.push(symbol_name(&mapping.source));
                    target_vec.push(symbol_name(&mapping.target));
                }
                stream_map.insert(
                    symbol_name(&definition.result),
                    map_val_struct {
                        relation: rename.relation.clone(),
                        source: source_vec,
                        target: target_vec,
                        is_rename: true,
                    },
                );
            }
            relational_plan::Operator::NaturalJoin(join) => {
                let mut new_source_vec: Vec<String> = Vec::new();
                let mut new_target_vec: Vec<String> = Vec::new();
                let mut shared_var: Vec<String> = Vec::new();

                /*
                eprintln!("NaturalJoin left: {:?}", join.left.to_string());

                eprintln!(
                    "left stream map relation : {}",
                    stream_map.get(&join.left.to_string()).unwrap().relation,
                );
                */
                let left_key = symbol_name(&join.left);
                let right_key = symbol_name(&join.right);

                let left_entry = match stream_map.get(&left_key) {
                    Some(entry) => entry,
                    None => {
                        return Err(syn::Error::new_spanned(join, "error"));
                    }
                };
                let right_entry = match stream_map.get(&right_key) {
                    Some(entry) => entry,
                    None => {
                        return Err(syn::Error::new_spanned(join, "error"));
                    }
                };

                // only a direct rename names a declared relation with real
                // column position, an intermediate join result has neither.

                if !right_entry.is_rename {
                    return Err(syn::Error::new_spanned(
                        join,
                        "each joins right op direct rename",
                    ));
                }

                for item in &left_entry.target {
                    if right_entry.target.contains(item) {
                        shared_var.push(item.to_string());
                    }
                }

                // Key is the relation's symbol name; the Ident rides along so the
                // emitted requirement keeps the original spelling.
                let mut m: BTreeMap<String, (syn::Ident, Vec<i32>)> = BTreeMap::new();

                for item in &shared_var {
                    for (positon, name) in right_entry.target.iter().enumerate() {
                        if item == name {
                            let relation = right_entry.relation.clone();

                            m.entry(symbol_name(&relation))
                                .or_insert_with(|| (relation, Vec::new()))
                                .1
                                .push(positon as i32);
                        }
                    }
                }

                // now we need to create a new stream map for the result of the join

                for item in &left_entry.target {
                    if shared_var.contains(item) {
                        continue;
                    }
                    new_target_vec.push(item.to_string());
                }

                for item in &shared_var {
                    new_target_vec.push(item.to_string());
                }

                for item in &right_entry.target {
                    if shared_var.contains(item) {
                        continue;
                    }
                    new_target_vec.push(item.to_string());
                }

                for (position, item) in new_target_vec.iter().enumerate() {
                    //eprintln!("{}->{} ", position, item);
                    new_source_vec.push(format!("c{}", position.to_string()));
                }

                stream_map.insert(
                    symbol_name(&definition.result),
                    map_val_struct {
                        relation: definition.result.clone(),
                        source: new_source_vec,
                        target: new_target_vec,
                        is_rename: false,
                    },
                );

                for x in m.iter() {
                    let rltn = &x.1.0;
                    let mut v = x.1.1.clone();
                    v.sort();

                    let mut columns: Vec<syn::Index> = Vec::new();
                    for position in &v {
                        columns.push(syn::Index::from(*position as usize));
                    }

                    // insert returns false when this exact index was already emitted.
                    if seen.insert((symbol_name(rltn), v.clone())) {
                        ans.push(quote! {#rltn[#(#columns),*];});
                    }
                }
            }
            _ => {}
        }

        //eprintln!("semi_token: {:?}", definition.semi_token);
    }

    let generated = quote! {
        #source
        indexes{
            #(#ans)*
        }
    };
    //eprintln!("{}", generated);
    syn::parse2(generated)
}

/// Basic Homework 3: lower the required indexed join to staged, ordinary Rust.
///
/// The returned file must contain the complete query API and concrete Rust
/// storage-building statements. Its core join is one
/// `::mini_linq::pull!` call whose body and head occurrences are paired with
/// their exact Rust patterns, sources, predicates, and output expression.
/// Every written index requirement, including an unused extra one, is built as
/// a concrete Rust collection. During its one source walk, this pass returns a
/// span-aware error if a canonical access requirement is missing.
#[allow(unreachable_code, clippy::diverging_sub_expression)]
#[contracts::contract(
    debug_requires(
        index_requirements::contract::well_formed(source),
        "source must satisfy the IndexRequirements local contract"
    ),
    debug_ensures(pass_contracts::physical_input_is_supported(source) -> ret.is_ok()),
    debug_ensures(!pass_contracts::physical_input_is_supported(source) -> ret.is_err()),
)]
pub fn compile_index_requirements(source: &index_requirements::Module) -> syn::Result<syn::File> {
    // source indexes
    // clear out the struct

    for declaration in &source.relational_plan.program.inputs {
        let m = &declaration.name;
        eprintln!(" relation: {}", quote!(#m));
    }

    for requirement in &source.indexes {
        let mut columns = Vec::new();

        for column in &requirement.key_columns {
            columns.push(column.index);
        }
        eprintln!("Index: {} {:?}", requirement.relation, columns);
    }

    for definition in &source.relational_plan.plan.definitions {
        eprintln!("Plan: {}", quote!(#definition));
    }
    /*
        for requirement in &source.indexes {
            for definition in &source.relational_plan.program.inputs {
                if (symbol_name(&requirement.relation) == symbol_name(&definition.name)) {
                    for (position, column) in definition.columns.iter().enumerate() {
                        let ty = &column.ty;
                        eprintln!("{:?} -> {:?}", position, column);
                        eprintln!("{}", quote!(#ty));
                        eprintln!("----------------");
                    }
                }
            }
        }
    */
    todo!("later homework: implement IndexRequirements -> staged Rust")
}

/// Supplied pull strategy: expose a conventional named iterator plan.
///
/// This pass chooses the source and binary operator tree, assigns every stream
/// a stable name, and records complete tuple bindings at every boundary. It
/// does not choose Rust iterator combinators or construct Rust closures.
#[contracts::contract(
    debug_requires(
        rust_access_plan::contract::well_formed(source),
        "source must satisfy the RustAccessPlan local contract"
    ),
    debug_ensures(pass_contracts::iterator_pipeline_is_exact(source, &ret))
)]
pub fn compile_pull(source: &rust_access_plan::Plan) -> iterator_pipeline::Pipeline {
    let mut binding = Vec::new();
    let mut clauses = source.body.iter();
    let mut definitions = Vec::<TokenStream>::new();
    let mut next_stream_number = 0usize;

    let mut current_stream = match clauses.next() {
        Some(
            clause @ rust_access_plan::Clause {
                access: rust_access_plan::RustAccess::For { .. },
                ..
            },
        ) => {
            let stream = next_stream(&mut next_stream_number);
            let definition = lower_root_scan(clause, &stream, &mut binding);
            definitions.push(definition);
            stream
        }
        Some(clause) => {
            let stream = next_stream(&mut next_stream_number);
            definitions.push(quote! {
                #stream = unit yield ();
            });
            let (next_stream, definition) =
                lower_operator(clause, &stream, &mut binding, &mut next_stream_number);
            definitions.push(definition);
            next_stream
        }
        None => {
            let stream = next_stream(&mut next_stream_number);
            definitions.push(quote! {
                #stream = unit yield ();
            });
            stream
        }
    };

    for clause in clauses {
        let (next_stream, definition) = lower_operator(
            clause,
            &current_stream,
            &mut binding,
            &mut next_stream_number,
        );
        definitions.push(definition);
        current_stream = next_stream;
    }

    let project_pattern = binding_pattern(&binding);
    let output = &source.head.output;
    let project_stream = next_stream(&mut next_stream_number);
    definitions.push(quote! {
        #project_stream = project #current_stream as #project_pattern yield (#output);
    });
    let distinct_stream = next_stream(&mut next_stream_number);
    definitions.push(quote! {
        #distinct_stream = distinct #project_stream;
    });

    syn::parse2(quote! {
        #(#definitions)*
        return #distinct_stream;
    })
    .expect("a well-formed RustAccessPlan must lower to an IteratorPipeline")
}

fn lower_root_scan(
    clause: &rust_access_plan::Clause,
    stream: &syn::Ident,
    binding: &mut Vec<syn::Ident>,
) -> TokenStream {
    let rust_access_plan::RustAccess::For {
        pattern, source, ..
    } = &clause.access
    else {
        unreachable!("the root scan caller selected a Rust `for` access")
    };
    binding.extend(pattern_variables(pattern));
    let output = binding_expression(binding);
    quote! {
        #stream = scan #pattern in (#source) yield #output;
    }
}

fn lower_operator(
    clause: &rust_access_plan::Clause,
    input_stream: &syn::Ident,
    binding: &mut Vec<syn::Ident>,
    next_stream_number: &mut usize,
) -> (syn::Ident, TokenStream) {
    let input = binding_pattern(binding);
    match &clause.access {
        rust_access_plan::RustAccess::For {
            pattern, source, ..
        } => {
            binding.extend(pattern_variables(pattern));
            let output = binding_expression(binding);
            let stream = next_stream(next_stream_number);
            let definition = quote! {
                #stream = join #input_stream as #input with #pattern in (#source) yield #output;
            };
            (stream, definition)
        }
        rust_access_plan::RustAccess::If { condition, .. } => {
            let stream = next_stream(next_stream_number);
            let definition = quote! {
                #stream = filter #input_stream as #input if (#condition);
            };
            (stream, definition)
        }
    }
}

fn next_result(next_result_number: &mut usize) -> syn::Ident {
    let result = format_ident!("r{}", *next_result_number, span = Span::mixed_site());
    *next_result_number += 1;
    result
}

fn next_stream(next_stream_number: &mut usize) -> syn::Ident {
    let stream = format_ident!("iter{}", *next_stream_number, span = Span::mixed_site());
    *next_stream_number += 1;
    stream
}

fn pattern_variables(pattern: &syn::Pat) -> impl Iterator<Item = syn::Ident> + '_ {
    let variables: Vec<syn::Ident> = match pattern {
        syn::Pat::Tuple(pattern) => pattern.elems.iter().map(simple_pattern_ident).collect(),
        syn::Pat::Ident(pattern) => vec![pattern.ident.clone()],
        _ => unreachable!(
            "the RustAccessPlan contract requires a simple tuple pattern for generated `for` accesses"
        ),
    };
    variables.into_iter()
}

fn simple_pattern_ident(element: &syn::Pat) -> syn::Ident {
    let syn::Pat::Ident(element) = element else {
        unreachable!("the RustAccessPlan contract requires simple identifier bindings")
    };
    element.ident.clone()
}

fn binding_pattern(binding: &[syn::Ident]) -> syn::Pat {
    syn::parse_quote!((#(#binding,)*))
}

fn binding_expression(binding: &[syn::Ident]) -> syn::Expr {
    syn::parse_quote!((#(#binding,)*))
}

/// Lower the named physical operators to the actual ordinary-Rust expression.
///
/// This is the final semantic lowering: it selects standard iterator adapters
/// and writes one Rust local per named operator. The returned [`syn::Expr`] is
/// already the target representation, so proc-macro emission is only
/// `ToTokens`; there is no later code generator that reconstructs the plan.
#[contracts::contract(
    debug_requires(
        iterator_pipeline::contract::well_formed(source),
        "source must satisfy the IteratorPipeline local contract"
    ),
    debug_ensures(pass_contracts::iterator_pipeline_rust_is_exact(source, &ret))
)]
pub fn compile_iterator_pipeline(source: &iterator_pipeline::Pipeline) -> syn::Expr {
    let locals = source
        .definitions
        .iter()
        .map(lower_operator_to_rust)
        .collect::<Vec<_>>();
    let return_stream = &source.return_stream.stream;
    syn::parse2(quote! {
        ::core::iter::Iterator::flatten(
            ::core::iter::once_with(move || {
                #(#locals)*
                #return_stream
            })
        )
    })
    .expect("a well-formed IteratorPipeline must lower to one Rust expression")
}

fn lower_operator_to_rust(definition: &iterator_pipeline::Definition) -> TokenStream {
    let stream = &definition.stream;
    match &definition.operator {
        iterator_pipeline::Operator::Unit(operator) => {
            let binding = &operator.binding;
            quote! {
                let #stream = ::core::iter::once(#binding);
            }
        }
        iterator_pipeline::Operator::Scan(operator) => {
            let pattern = &operator.item_pattern;
            let source = &operator.source;
            let binding = &operator.binding;
            quote! {
                let #stream = ::core::iter::Iterator::map(
                    ::core::iter::IntoIterator::into_iter(#source),
                    move |#pattern| #binding,
                );
            }
        }
        iterator_pipeline::Operator::Join(operator) => {
            let input_stream = &operator.input_stream;
            let input_pattern = &operator.input_pattern;
            let item_pattern = &operator.item_pattern;
            let source = &operator.source;
            let binding = &operator.binding;
            quote! {
                let #stream = ::core::iter::Iterator::flat_map(
                    #input_stream,
                    move |#input_pattern| {
                        ::core::iter::Iterator::map(
                            ::core::iter::IntoIterator::into_iter(#source),
                            move |#item_pattern| #binding,
                        )
                    },
                );
            }
        }
        iterator_pipeline::Operator::Filter(operator) => {
            let input_stream = &operator.input_stream;
            let input_pattern = &operator.input_pattern;
            let borrowed_pattern: syn::Pat = syn::parse_quote!(&#input_pattern);
            let condition = &operator.condition;
            quote! {
                let #stream = ::core::iter::Iterator::filter(
                    #input_stream,
                    move |#[allow(unused_variables)] #borrowed_pattern| #condition,
                );
            }
        }
        iterator_pipeline::Operator::Project(operator) => {
            let input_stream = &operator.input_stream;
            let input_pattern = &operator.input_pattern;
            let output = &operator.output;
            quote! {
                let #stream = ::core::iter::Iterator::map(
                    #input_stream,
                    move |#[allow(unused_variables)] #input_pattern| #output,
                );
            }
        }
        iterator_pipeline::Operator::Distinct(operator) => {
            let input_stream = &operator.input_stream;
            quote! {
                let #stream = {
                    let mut seen = ::std::collections::HashSet::new();
                    ::core::iter::Iterator::filter(
                        #input_stream,
                        move |row| seen.insert(::core::clone::Clone::clone(row)),
                    )
                };
            }
        }
    }
}
