# Modern Query Processing homework

This is the student code repository for **Modern Query Processing, Fall 2026**.
The project follows one query from a conjunctive query (CQ), through typed
intermediate representations, to executable Rust.

Queries, relational outputs, access plans, and iterator returns end with `;`.
The earlier period terminator is rejected. Syntax types derive parsing and
token emission; declarations and definitions declare their repetition on the
corresponding fields.

- [Course website](https://pldi.me/modern-query-processing-notes/)
- [Project contract](https://pldi.me/modern-query-processing-notes/project.html)
- [Homework repository](https://github.com/StarGazerM/modern-query-processing-homework)

## Start Homework 1

Clone the repository, start from the stable `hw1` branch, and create your own
submission branch:

```text
git clone https://github.com/StarGazerM/modern-query-processing-homework.git
cd modern-query-processing-homework
git switch hw1
git switch -c submission/hw1
```

Read [Homework 1: CQ to RelationalPlan](doc/HW1-START-HERE.md). Edit only
`compile_cq` in `crates/cq-compiler/src/passes.rs`; the other two unfinished
functions belong to later homework branches.

These commands work before any homework function is implemented:

```text
cargo run --example cq-inspection
cargo test -p cq-ir --test relational_plan
cargo xtask list
cargo xtask explain triangle --through cq
cargo test --workspace --locked
```

The focused Homework 1 test begins ignored because the starter intentionally
contains `todo!()`:

```text
cargo test -p cq-compiler --test pass_contracts cq_to_relational_plan_contract -- --ignored
```

## Release branches

Each announced homework has an immutable starter branch. The default `main`
branch points to the latest common starter, while the numbered branch remains a
stable reference for that assignment.

| Branch | Released content |
|---|---|
| `hw1` | Implement `CQ -> RelationalPlan` |
| `hw2` | Released after HW1; includes the HW1 reference checkpoint and the HW2 starter |
| `hw3` | Released after HW2; includes the HW2 reference checkpoint and the HW3 starter |
| `pick-one/<name>` | Released after the common core for one approved advanced extension |

Only `main` and `hw1` are public initially. A later starter branch is published
only when its prerequisite reference solution may be released. Do not base a
submission on an unannounced branch. See [Branch and release policy](BRANCHES.md).

## Common compiler path

```text
CQ
  -- HW1 --> RelationalPlan
  -- HW2 --> IndexRequirements { complete RelationalPlan + keys }
  -- HW3 --> staged Rust file containing pull! { RustAccessPlan }
  -- supplied pull --> staged Rust file containing iterator_pipeline! { IteratorPipeline }
  -- supplied Rust lowering --> ordinary Rust with one lazy result-set Iterator
```

Staff supplies parsers, typed syntax definitions, local and cross-stage
contracts, macro routing, the two pull lowerings, fixtures, and the structural
and runtime harness. Students implement the named transformations rather than
building a parser, DBMS, optimizer, or runtime from scratch.

The cumulative [query-lowering guide](doc/R1-START-HERE.md) and
[language contract](doc/DSL.md) document all six boundaries. Homework 1 needs
only the first two complete artifacts; later material is reference, not current
required reading.

## Required advanced choice: Pick 1

After the common core, every team completes **one** released advanced
extension. Planned extension families are:

- Ascent-style safe negation and aggregation;
- Tokio channel execution;
- incremental maintenance with explicit progress;
- Rayon parallel physical lowering; or
- exact index sharing across several queries.

Teams submit ranked preferences. Because the class is expected to have fewer
than 20 students, the instructor will balance assignments so different teams
explore different extensions when practical. If several teams use one family,
they will use different workloads, claims, or adversarial cases.

An extension counts only after staff publishes its `pick-one/<name>` branch
with the target IR/runtime shell, complete example, tests, and a verified
reference implementation. Combining several extensions is not required. The
[advanced-extension design](doc/OPTIONAL-PROJECT-TRACKS.md) records the branch
points and staff obligations.

## Evidence and AI-assisted work

AI-assisted coding, testing, debugging, and writing are allowed. Every release
must complete [AI-USE.md](AI-USE.md) and include an invariant, an adversarial
case, an adjacent-stage trace, and any material AI correction or rejection.
Prompt transcripts are not required. Students remain responsible for the code
and must be able to explain an unseen case individually.

## Additional examples and verification

- [Example map](examples/README.md)
- [Workload and DuckDB guide](workloads/README.md)
- [Documentation map](doc/README.md)

After all three common passes are complete, the project can compile and compare
the full workload corpus:

```text
cargo check -p cq-workloads --features compiled-queries --locked
cargo xtask differential all tiny --scenario all
```

Enabling the feature before all three passes return `Ok` fails with many
errors in files the homework never edits. Each `workload_query!` invocation
runs the three passes at expansion time, so an unimplemented `todo!` panics
inside the proc macro, that invocation emits no items, and the catalog then
reports missing `module` and `RUST_PATH` values across the whole corpus. The
incomplete pass is not named in that output. Use
`cargo xtask explain <case> --through <stage>` until the passes are finished.

The checked-in toolchain pins Rust 1.97.1 with `rustfmt` and `clippy`.
