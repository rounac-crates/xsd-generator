# XSD generator
A custom bindings generator for XML schema documents (XSD). The library part is
a fairly generalized XSD parser that gives parsed `Element` structures to the
user; `Element` is not exclusive to `xs:element`. **Include statements are not
handled, so every schema file must be given as an argument if desired**.

The main binary has little schema-specific tailoring, and it is for the
Universal C2 Interface (UCI) `UniversallyUniqueIdentifierType` to use
`uuid::Uuid` over a string for ergonomics.

# Generated code style
The generator creates `Element`s for:

- `xs:schema`
- `xs:simpleType`
- `xs:complexType`
- `xs:element`
- `xs:enumeration`

The `output_fn` parameter in `parse_schema` is only called for:

- `xs:schema`
- `xs:simpleType`
- `xs:complexType`

Elements and enumerations are added to an ancestor element, usually their
immediate parent.

While `xs:attribute` is ignored, any attributes within tags given to
`output_fn` are stored in the `Element.attrs` map.

## xs:schema
There is only one schema element per file and it will only contain annotations
and top-level `xs:element`s.

The schema annotations will be output in `lib.rs` just below a title docstring
made from the crate name. All schema elements will be placed in the
`elements.rs` module.

## xs:simpleType
Simple types currently map to enums, newtype structs, or aliases in that order.
The enum determination is trivial, but a newtype vs alias is based on whether
there is a custom serde implementation needed. Since all non-enumeration
restrictions are presently not used (but still documented), types aliases make
sense for ergonomics. If/when the other restrictions are used, the desire is to
keep type aliases to avoid the need for `.try_into()` or similar everywhere.

Enums are placed in the `enums.rs` module, with the newtypes and aliases going
in `common.rs`.

## xs:complexType
Complex types currently map `xs:sequence` and `xs:choice` to structs and enums,
respectively. Struct mapping is very simple 1-to-1 of the child elements to
members, whereas choice elements become enums with newtype variants; struct
variants are not presently handled but largely supported in the code.

Abstract types are initially handled as structs but are later converted to
choice enums where the variants are all types in the schema that inherit from
it. While this does not allow for true abstract/polymorphic type usage, it is
the simplest way to provide the minimal expected functionality; true
polymorphic behavior is very difficult especially when using serde. For any
abstract type that is not used in the schema, it becomes a unit struct.

Structs are placed in the `types.rs` module and choice enums are placed in the
`choices.rs` module, except for the abstract type choice enums which remain in
`types.rs` due to implementation constraints.

## xs:extension
Any type that extends another will have the "parent" fields manually flattened
into itself in the appropriate order (depth-first top-down). This is due to
issues with `#[serde(flatten)]` for [quick-xml][1] specifically, but
potentially other formats as well. In addition, "mirror traits" (traits that
mirror the contents of a type) are created for all structs in order to mimic
inheritance and improve ergonomics.

The mirror traits are placed in the `traits.rs` module


[1]: https://github.com/tafia/quick-xml/issues/286
