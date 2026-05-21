# Diplomat HIR Example: `Counter`

This file is a readable, editor-friendly HIR sketch for a tiny Diplomat API.

Important: Diplomat does not normally persist HIR to disk. The real HIR is built in memory as a `diplomat_core::hir::TypeContext` inside `diplomat-tool`, then passed directly to a backend such as C, C++, JS, Kotlin, Dart, or nanobind.

The names below match the real HIR data structures, but some low-signal fields such as `docs`, most `attrs`, and exact lifetime internals are omitted so the shape is easy to inspect.

## Source

```rust
#[diplomat::bridge]
mod ffi {
    #[diplomat::opaque]
    pub struct Counter(u32);

    impl Counter {
        pub fn new(value: u32) -> Box<Counter> {
            Box::new(Counter(value))
        }

        pub fn get(&self) -> u32 {
            self.0
        }

        pub fn increment(&mut self) {
            self.0 += 1;
        }
    }
}
```

## AST Shape

The AST is source-shaped. It records what appeared inside the `#[diplomat::bridge]` module.

```text
Module {
    name: "ffi",
    declared_types: {
        "Counter": CustomType::Opaque(
            OpaqueType {
                name: "Counter",
                mutability: Immutable,
                methods: [
                    Method {
                        name: "new",
                        abi_name: "Counter_new",
                        self_param: None,
                        params: [
                            Param {
                                name: "value",
                                ty: TypeName::Primitive(U32),
                            },
                        ],
                        return_type: Some(
                            TypeName::Box(
                                TypeName::Named(PathType("Counter"))
                            )
                        ),
                    },
                    Method {
                        name: "get",
                        abi_name: "Counter_get",
                        self_param: Some(SelfParam("&self")),
                        params: [],
                        return_type: Some(TypeName::Primitive(U32)),
                    },
                    Method {
                        name: "increment",
                        abi_name: "Counter_increment",
                        self_param: Some(SelfParam("&mut self")),
                        params: [],
                        return_type: None,
                    },
                ],
            }
        )
    },
}
```

## HIR Shape

The HIR is backend-shaped. It resolves names into IDs and turns Rust syntax like `Box<Counter>`, `&self`, and `&mut self` into FFI semantics.

```text
TypeContext {
    out_structs: [],
    structs: [],
    opaques: [
        OpaqueDef {
            // This is the first opaque in TypeContext. Later references use OpaqueId(0).
            name: "Counter",
            dtor_abi_name: "Counter_destroy",
            lifetimes: LifetimeEnv {},
            special_method_presence: SpecialMethodPresence {
                constructor: true,
                // Other special methods omitted.
            },
            methods: [
                Method {
                    name: "new",
                    abi_name: "Counter_new",
                    param_self: None,
                    params: [
                        Param {
                            name: "value",
                            ty: Type<InputOnly>::Primitive(PrimitiveType::U32),
                        },
                    ],
                    output: ReturnType<OutputOnly>::Infallible(
                        SuccessType::OutType(
                            Type<OutputOnly>::Opaque(
                                OpaquePath {
                                    tcx_id: OpaqueId(0),
                                    owner: MaybeOwn::Own,
                                    optional: false,
                                    lifetimes: [],
                                }
                            )
                        )
                    ),
                },

                Method {
                    name: "get",
                    abi_name: "Counter_get",
                    param_self: Some(
                        ParamSelf {
                            ty: SelfType::Opaque(
                                OpaquePath {
                                    tcx_id: OpaqueId(0),
                                    owner: Borrow {
                                        mutability: Immutable,
                                        lifetime: anonymous_method_lifetime,
                                    },
                                    optional: false,
                                    lifetimes: [],
                                }
                            ),
                        }
                    ),
                    params: [],
                    output: ReturnType<OutputOnly>::Infallible(
                        SuccessType::OutType(
                            Type<OutputOnly>::Primitive(PrimitiveType::U32)
                        )
                    ),
                },

                Method {
                    name: "increment",
                    abi_name: "Counter_increment",
                    param_self: Some(
                        ParamSelf {
                            ty: SelfType::Opaque(
                                OpaquePath {
                                    tcx_id: OpaqueId(0),
                                    owner: Borrow {
                                        mutability: Mutable,
                                        lifetime: anonymous_method_lifetime,
                                    },
                                    optional: false,
                                    lifetimes: [],
                                }
                            ),
                        }
                    ),
                    params: [],
                    output: ReturnType<OutputOnly>::Infallible(
                        SuccessType::Unit
                    ),
                },
            ],
        },
    ],
    enums: [],
    traits: [],
    functions: [],
}
```

## What A C Backend Reads From This HIR

The C adapter sees the HIR semantics and maps them to C ABI declarations:

```text
OpaqueDef Counter
    -> typedef struct Counter Counter;

Method new
    output = owned opaque Counter
    -> Counter* Counter_new(uint32_t value);

Method get
    self = immutable borrowed opaque Counter
    output = u32
    -> uint32_t Counter_get(const Counter* self);

Method increment
    self = mutable borrowed opaque Counter
    output = unit
    -> void Counter_increment(Counter* self);

Opaque destructor
    -> void Counter_destroy(Counter* self);
```

## Why This Is Not Normally On Disk

HIR is an internal intermediate representation:

```text
syn::File
  -> diplomat_core::ast::File / Env
  -> diplomat_core::hir::TypeContext
  -> backend::run(...)
  -> generated files
```

The generated files are persisted. The HIR is not. To persist real HIR, the project would need a debug command or a small developer utility that parses an entry file, calls `TypeContext::from_syn`, and writes a `Debug` or custom formatted dump.
