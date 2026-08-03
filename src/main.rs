//! Program which takes in UCI XSD files and creates a crate.
//!

mod contents;
mod impls;
mod names;
mod traits;

use contents::*;
use names::*;
use std::{
	collections::{BTreeSet, HashMap, hash_map},
	env,
	fmt::Write as _,
	path::{Path, PathBuf},
	rc::Rc,
};
use traits::{Trait, add_mirror_traits};
use xsd_generator::{BoundType, ContentType, Element, ExtRest, parse_schema};

use crate::{
	contents::serde_utils::{add_custom_serde, has_custom_serde},
	impls::{impl_attrs, impls_for_types},
	traits::impl_mirror_trait_fn,
};

static CHOICE_DERIVES: &[&str] = &["Clone", "Debug", "PartialEq"];
static ENUM_DERIVES: &[&str] = &[
	"Clone",
	"Copy",
	"Debug",
	"Deserialize",
	"PartialEq",
	"Serialize",
];
static NON_ENUM_DERIVES: &[&str] = &[
	"Clone",
	"Debug",
	"Default",
	"Deserialize",
	"PartialEq",
	"Serialize",
];

/// Add the schema-level element (just documentation), assumed to be first 0.
fn add_schema(schema: &Element, out_crate: &mut Crate) {
	let contents = out_crate
		.contents
		.entry(PathBuf::from("crate"))
		.or_default();

	_ = write!(contents.docs, "{}\n\n", schema.annotation);
	contents.modules.insert("#![allow(dead_code)]".to_owned());

	if let Some(v) = schema.attrs.get("version") {
		contents.imports.insert(format!(
			"pub const SCHEMA_VERSION: &'static str = r#\"{v}\"#;"
		));
	}
}

fn add_choices(elems: &[Element], cr8: &mut Crate) {
	let choices = elems
		.iter()
		.filter(|e| e.type_.as_ref().is_some_and(|t| *t == ContentType::Choice));

	// Export module in library.
	let lib = cr8.contents.entry(PathBuf::from("crate")).or_default();
	lib.modules.insert(String::from("pub mod choices;"));

	// Create module for contents.
	let crate_path = PathBuf::from("crate/choices");
	let contents = cr8.contents.entry(crate_path.clone()).or_default();
	_ = write!(contents.docs, "Module for all choice types.\n\n");

	for choice in choices {
		// Type must be pascal.
		let mut choice_name = match to_pascal_case(&choice.name) {
			Some(n) => n,
			None => choice.name.clone(),
		};
		fix_illegal_name(&mut choice_name);

		// Parent (MUST HAVE NO TYPE)
		let parent = match choice.ext_rest {
			Some(ExtRest::Extend(ref p)) => {
				// Type must be pascal.
				let mut pname = match to_pascal_case(p) {
					Some(n) => n,
					None => p.clone(),
				};
				fix_illegal_name(&mut pname);

				Some(Rc::from(pname))
			}
			_ => None,
		};

		let mut variants = Vec::new();
		for e in choice.values.iter() {
			// Determine plurality
			let optional = e.min == 0;
			let list = e.max > 1;
			let plurality = if list {
				FieldPlurality::Plural
			} else if optional {
				FieldPlurality::Optional
			} else {
				FieldPlurality::None
			};

			// Type name must be pascal
			let choice_tn = e.type_name.as_ref().unwrap();
			let mut tn = match to_pascal_case(choice_tn) {
				Some(n) => n,
				None => choice_tn.clone(),
			};
			fix_illegal_name(&mut tn);

			// Map type before custom serde.
			map_type(&mut tn);

			let mut c = Field {
				name: String::new(),
				typename: tn,
				public: true,
				plurality,
				boxed: false,
				docs: String::new(),
				rename: None,
				attrs: Vec::new(),
			};
			add_custom_serde(&mut c);

			// Variant name must be pascal.
			let mut vnt_name = match to_pascal_case(&e.name) {
				Some(n) => n,
				None => e.name.clone(),
			};
			fix_illegal_name(&mut vnt_name);

			let vnt = Variant {
				name: Rc::from(vnt_name.as_str()),
				vtype: VariantType::Newtype(c),
				docs: e.annotation.clone(),
				rename: None,
				attrs: Vec::new(),
			};

			variants.push(vnt);
		}

		// Construct the macro string.
		let mut separated_variants = String::new();
		for (vnt, orig) in variants.iter().zip(choice.values.iter()) {
			let VariantType::Newtype(ref field) = vnt.vtype else {
				unreachable!()
			};

			// Check for the serde_with
			if !field.attrs.is_empty() {
				// Extract module name
				let mut quotes = field.attrs[0].match_indices('"');
				let start = quotes.next().unwrap().0 + 1;
				let end = quotes.next().unwrap().0;
				let module = &field.attrs[0][start..end];
				_ = write!(separated_variants, "\t#[{} => {module}]\n", field.typename);
			}

			// Format is: variant_name -> serialized_variant_name
			_ = write!(separated_variants, "\t{} -> \"{}\",\n", vnt.name, orig.name);
		}
		let mut serde_macro_impl = String::new();
		_ = write!(
			serde_macro_impl,
			"struct_like_serde! {{\n\t{}\n",
			choice_name
		);
		_ = write!(serde_macro_impl, "{separated_variants}");
		_ = write!(serde_macro_impl, "}}");

		let impls = vec![serde_macro_impl];

		let mut c = RustType {
			name: Rc::from(choice_name),
			output: true,
			docs: choice.annotation.clone(),
			rename: None,
			public: true,
			attrs: Vec::new(),
			derive: CHOICE_DERIVES.iter().map(|&v| v.to_owned()).collect(),
			any_features: BTreeSet::new(),
			extends: parent,
			members: TypeMembers::Variants(variants),
			impls,
		};
		rustify_rtype(&mut c);

		cr8.type_map.insert(c.name.clone(), crate_path.clone());
		contents.types.insert(c.name.clone(), c);
	}
}

fn add_enum_types(elems: &[Element], cr8: &mut Crate) {
	let enums = elems.iter().filter(|&e| {
		e.ext_rest.as_ref().is_some_and(|v| {
			if let ExtRest::Restrict(r) = v {
				!r.enumeration.is_empty()
			} else {
				false
			}
		})
	});

	// Export module in library.
	let lib = cr8.contents.entry(PathBuf::from("crate")).or_default();
	lib.modules.insert(String::from("pub mod enums;"));

	// Create module for contents.
	let crate_path = PathBuf::from("crate/enums");
	let contents = cr8.contents.entry(crate_path.clone()).or_default();
	_ = write!(contents.docs, "Module for all enumerated types.\n\n");
	contents
		.imports
		.insert("use serde::{Deserialize, Serialize};".to_string());

	for en in enums {
		let ExtRest::Restrict(r) = en.ext_rest.as_ref().unwrap() else {
			unreachable!()
		};

		let mut en_rename = None;

		// Type must be pascal.
		let mut en_name = match to_pascal_case(&en.name) {
			Some(n) => {
				en_rename = Some(en.name.as_str().into());
				n
			}
			None => en.name.clone(),
		};
		fix_illegal_name(&mut en_name);

		let mut variants = Vec::new();
		for variant in r.enumeration.iter() {
			// Variant name must be pascal.
			let mut rename = None;
			let mut vnt_name = match to_pascal_case(&variant.value) {
				Some(n) => {
					rename = Some(variant.value.as_str().into());
					n
				}
				None => variant.value.clone(),
			};
			fix_illegal_name(&mut vnt_name);

			let vnt = Variant {
				name: Rc::from(vnt_name.as_str()),
				vtype: VariantType::Unit,
				docs: variant.annotation.clone(),
				rename,
				attrs: Vec::new(),
			};

			variants.push(vnt);
		}

		let mut e = RustType {
			name: Rc::from(en_name),
			output: true,
			docs: en.annotation.clone(),
			rename: en_rename,
			public: true,
			attrs: Vec::new(),
			derive: ENUM_DERIVES.iter().map(|&v| v.to_owned()).collect(),
			any_features: BTreeSet::new(),
			extends: None,
			members: TypeMembers::Variants(variants),
			impls: Vec::new(),
		};
		rustify_rtype(&mut e);

		cr8.type_map.insert(e.name.clone(), crate_path.clone());
		contents.types.insert(e.name.clone(), e);
	}
}

fn add_complex_structs(elems: &[Element], cr8: &mut Crate) {
	let structs = elems
		.iter()
		.filter(|e| e.type_.as_ref().is_some_and(|t| *t == ContentType::Complex));

	// Export module in library.
	let lib = cr8.contents.entry(PathBuf::from("crate")).or_default();
	lib.modules.insert(String::from("pub mod types;"));

	// Create module for contents.
	let crate_path = PathBuf::from("crate/types");
	let contents = match cr8.contents.entry(crate_path.clone()) {
		hash_map::Entry::Vacant(v) => {
			let mut e = v.insert_entry(Module::default());
			_ = write!(e.get_mut().docs, "Module for all struct types.\n\n");
			e.get_mut()
				.imports
				.insert("use serde::{Deserialize, Serialize};".to_string());

			e.into_mut()
		}
		hash_map::Entry::Occupied(o) => o.into_mut(),
	};

	for st in structs {
		let mut fields = Vec::new();
		let mut st_rename = None;

		// Type must be pascal.
		let mut st_name = match to_pascal_case(&st.name) {
			Some(n) => {
				st_rename = Some(st.name.as_str().into());
				n
			}
			None => st.name.clone(),
		};
		fix_illegal_name(&mut st_name);

		// Parent "field"
		let parent = match st.ext_rest {
			Some(ExtRest::Extend(ref p)) => {
				// Type must be pascal.
				let mut pname = match to_pascal_case(p) {
					Some(n) => n,
					None => p.clone(),
				};
				fix_illegal_name(&mut pname);

				Some(Rc::from(pname))
			}
			_ => None,
		};

		// Output remaining contents
		for field in st.values.iter() {
			let mut field_attrs = Vec::new();
			let mut field_rename = None;

			// Fix type name
			let mut field_name = match to_snake_case(&field.name) {
				Some(n) => {
					field_rename = Some(field.name.as_str().into());
					n
				}
				None => field.name.clone(),
			};
			fix_illegal_name(&mut field_name);

			// Prepare full type
			let mut type_name = match field.type_name {
				Some(ref name) => {
					let mut tn = match to_pascal_case(name) {
						Some(n) => n,
						None => name.clone(),
					};
					fix_illegal_name(&mut tn);

					tn
				}
				None => String::from("()"),
			};

			// Determine plurality
			let optional = field.min == 0;
			let list = field.max > 1;
			let plurality = if list {
				field_attrs.push("#[serde(skip_serializing_if = \"Vec::is_empty\")]".to_owned());
				field_attrs.push("#[serde(default)]".to_owned());
				FieldPlurality::Plural
			} else if optional {
				field_attrs.push("#[serde(skip_serializing_if = \"Option::is_none\")]".to_owned());
				field_attrs.push("#[serde(default)]".to_owned());
				FieldPlurality::Optional
			} else {
				FieldPlurality::None
			};

			// Check for custom serde after mapping field name
			map_type(&mut type_name);

			// Output field
			let mut f = Field {
				name: field_name,
				public: true,
				docs: field.annotation.clone(),
				rename: field_rename,
				typename: type_name,
				plurality,
				boxed: false,
				attrs: field_attrs,
			};
			add_custom_serde(&mut f);

			fields.push(f);
		}

		let mut t = RustType {
			name: Rc::from(st_name),
			output: !st.abstract_, // Do not output abstract types.
			public: true,
			docs: st.annotation.clone(),
			rename: st_rename,
			attrs: Vec::new(),
			derive: NON_ENUM_DERIVES.iter().map(|&v| v.to_owned()).collect(),
			any_features: BTreeSet::new(),
			extends: parent,
			members: TypeMembers::Fields(fields),
			impls: Vec::new(),
		};
		impl_attrs(&mut t, &st.attrs);
		//rustify_rtype(&mut t);

		cr8.type_map.insert(t.name.clone(), crate_path.clone());
		contents.types.insert(t.name.clone(), t);
	}
}

fn add_simple_structs(elems: &[Element], cr8: &mut Crate) {
	let structs = elems
		.iter()
		.filter(|e| e.type_.as_ref().is_some_and(|t| *t == ContentType::Simple))
		.filter(|e| {
			if let Some(ExtRest::Restrict(rest)) = e.ext_rest.as_ref() {
				rest.enumeration.len() == 0
			} else {
				false
			}
		});

	// Export module in library.
	let lib = cr8.contents.entry(PathBuf::from("crate")).or_default();
	lib.modules.insert(String::from("pub mod common;"));

	// Create module for contents.
	let crate_path = PathBuf::from("crate/common");
	let contents = cr8.contents.entry(crate_path.clone()).or_default();
	_ = write!(contents.docs, "Module with basic types.\n\n");
	contents
		.imports
		.insert("use serde::{Deserialize, Serialize};".to_string());

	for st in structs {
		let Some(ExtRest::Restrict(ref r)) = st.ext_rest else {
			panic!("Simple types must have a restriction");
		};

		let mut restriction_docs = String::new();

		// Create documentation for any restrictions
		if let Some(ref p) = r.pattern {
			_ = write!(restriction_docs, "* Pattern: `{p}`\n");
		}
		if let Some(ref l) = r.length {
			_ = write!(restriction_docs, "* Length: `{l}`\n");
		}
		if let Some(ref m) = r.min_length {
			_ = write!(restriction_docs, "* Minimum length: `{m}`\n");
		}
		if let Some(ref m) = r.max_length {
			_ = write!(restriction_docs, "* Maximum length: `{m}`\n");
		}
		if let Some(ref m) = r.min {
			match m {
				BoundType::Exclusive(x) => {
					_ = write!(restriction_docs, "* Minimum value: `{x}` (Exclusive)\n")
				}
				BoundType::Inclusive(x) => {
					_ = write!(restriction_docs, "* Minimum value: `{x}` (Inclusive)\n")
				}
			};
		}
		if let Some(ref m) = r.max {
			match m {
				BoundType::Exclusive(x) => {
					_ = write!(restriction_docs, "* Maximum value: `{x}` (Exclusive)\n")
				}
				BoundType::Inclusive(x) => {
					_ = write!(restriction_docs, "* Maximum value: `{x}` (Inclusive)\n")
				}
			};
		}

		// If any restrictions were documented above, add header then insert to docs.
		let mut docs = st.annotation.clone();
		if !restriction_docs.is_empty() {
			restriction_docs.insert_str(0, "\n\n# Restrictions\n");
			docs.push_str(&restriction_docs);
		}

		// Type must be pascal.
		let mut st_rename = None;
		let mut st_name = match to_pascal_case(&st.name) {
			Some(n) => {
				st_rename = Some(st.name.as_str().into());
				n
			}
			None => st.name.clone(),
		};
		fix_illegal_name(&mut st_name);

		let mut r_name = match to_pascal_case(&r.base) {
			Some(n) => n,
			None => r.base.clone(),
		};
		fix_illegal_name(&mut r_name);
		map_type(&mut r_name); // This happens early because of [has_custom_serde].

		let t = match contents::serde_utils::has_custom_serde(&r_name) {
			Some(s) => {
				let field = Field {
					name: String::new(),
					typename: r_name.clone(),
					public: true,
					plurality: FieldPlurality::None,
					boxed: false,
					docs: String::new(),
					rename: None,
					attrs: vec![format!("#[serde(with = \"{s}\")]")],
				};

				RustType {
					name: Rc::from(st_name),
					output: true,
					public: true,
					docs,
					rename: st_rename,
					attrs: Vec::new(),
					derive: NON_ENUM_DERIVES.iter().map(|&v| v.to_owned()).collect(),
					any_features: BTreeSet::new(),
					extends: None,
					members: TypeMembers::Fields(vec![field]),
					impls: Vec::new(),
				}
			}
			None => {
				RustType {
					name: Rc::from(st_name),
					output: true,
					public: true,
					docs,
					rename: None,
					attrs: Vec::new(), // Ignore all attrs from above
					derive: Vec::new(),
					any_features: BTreeSet::new(),
					extends: None,
					members: TypeMembers::Alias(r_name),
					impls: Vec::new(),
				}
			}
		};

		cr8.type_map.insert(t.name.clone(), crate_path.clone());
		contents.types.insert(t.name.clone(), t);
	}
}

/// Add elements present at the root level of the schema.
fn add_schema_elements(elems: &[Element], cr8: &mut Crate) {
	let elements = elems.iter().filter(|e| e.type_.as_ref().is_none());

	// Export module in library.
	let lib = cr8.contents.entry(PathBuf::from("crate")).or_default();
	lib.modules.insert(String::from("pub mod elements;"));

	// Create module for contents.
	let crate_path = PathBuf::from("crate/elements");
	let contents = cr8.contents.entry(crate_path.clone()).or_default();
	_ = write!(contents.docs, "Module with schema-level elements.\n\n");
	contents
		.imports
		.insert("use serde::{Deserialize, Serialize};".to_string());

	//let mut all_element_features = Vec::new();
	for elem in elements {
		let elem_type = elem.type_name.as_ref().unwrap();
		let mut tn = match to_pascal_case(elem_type) {
			Some(n) => n,
			None => elem_type.clone(),
		};
		fix_illegal_name(&mut tn);

		let f = Field {
			name: String::new(),
			public: true,
			typename: tn,
			plurality: FieldPlurality::None,
			boxed: false,
			attrs: vec!["#[serde(flatten)]".to_string()],
			rename: None,
			docs: String::new(),
		};

		// Type must be pascal.
		let mut rename = None;
		let mut elem_name = match to_pascal_case(&elem.name) {
			Some(n) => {
				rename = Some(elem.name.as_str().into());
				n
			}
			None => elem.name.clone(),
		};
		fix_illegal_name(&mut elem_name);

		// Name for feature of this element.
		let mut snake_name = match to_snake_case(&elem.name) {
			Some(n) => n,
			None => elem.name.clone(),
		};
		fix_illegal_name(&mut snake_name);

		let r = RustType {
			name: Rc::from(elem_name),
			output: true,
			public: true,
			docs: elem.annotation.clone(),
			rename,
			attrs: Vec::new(),
			derive: NON_ENUM_DERIVES.iter().map(|&v| v.to_owned()).collect(),
			any_features: BTreeSet::new(),
			extends: None,
			members: TypeMembers::Fields(vec![f]),
			impls: Vec::new(),
		};

		cr8.type_map.insert(r.name.clone(), crate_path.clone());
		contents.types.insert(r.name.clone(), r);

		// Add to list for "all" feature.
		//all_element_features.push(snake_name);
	}

	// Combine all features into a convenient "all" feature.
	/*let mut all_string = format!("all = [\n");
	for feat in all_element_features {
		_ = write!(all_string, "\t\"{feat}\",\n");
		cr8.features.push(format!("{feat} = []"));
	}
	all_string.push(']');
	cr8.features.push(all_string);*/
}

/// Remaps any type present in `rtype` using [map_type].
fn rustify_rtype(rtype: &mut RustType) {
	match rtype.members {
		TypeMembers::Alias(ref mut s) => _ = map_type(s),
		TypeMembers::Fields(ref mut fields) => {
			for field in fields.iter_mut() {
				map_type(&mut field.typename);
			}
		}
		TypeMembers::Variants(ref mut variants) => {
			for variant in variants.iter_mut() {
				if let VariantType::Newtype(ref mut f) = variant.vtype {
					map_type(&mut f.typename);
				}
			}
		}
	};
}

/// Modify type to another type if desired, returns `true` if modified.
///
/// Primarily intended to map XSD native types to Rust native types.
// TODO: Add a config/input file that allows user-specified type mappings, only
//       for the non-primitive; or make the current primitive mappings the
//       defaults. Easy to use (serde) into/try_from, but with will be harder
//       since those modules are all hard-coded right now; would need dynamic
//       file/module inclusion logic.
fn map_type(rtype: &mut String) -> bool {
	macro_rules! map_type_match {
		{$($from_type:ident => $type_1:ty)*} => {
			match rtype.as_str() {
				$(stringify!($from_type) => {
					rtype.truncate(0);
					_ = rtype.write_str(stringify!($type_1));
					true
				})*
				_ => false,
			}
		};
	}

	map_type_match! {
		// Native type mappings
		boolean => bool
		double => f64
		float => f32
		hexBinary => String
		byte => i8
		short => i16
		int => i32
		long => i64
		string => String
		unsignedByte => u8
		unsignedShort => u16
		unsignedInt => u32
		unsignedLong => u64

		// Requirement per CAL-016024
		integer => i64
		// Requirement per CAL-016027
		duration => i64
		// Requirement per CAL-016028
		dateTime => i64
		// Requirement per CAL-016029
		time => i64

		// UCI schema-specific type mappings for ergonomics.
		DateTimeType => chrono::DateTime<chrono::Utc>
		DurationType => chrono::TimeDelta
		TimeType => chrono::NaiveTime
		UniversallyUniqueIdentifierType => uuid::Uuid
	}
}

/// Go through all types to ensure only types eligible for Default derive it.
fn correct_defaults(cr8: &mut Crate) {
	// A map that will cache conclusions about a types Default eligibility.
	let mut default_types: HashMap<&str, bool> = HashMap::new();

	// Iterate every type to check eligibility.
	for typ in cr8.type_map.keys() {
		let Some(deps) = cr8.get_deps(typ, true) else {
			continue;
		};

		// If `typ` does not have default, no need to iterate remaining deps.
		if let Some(ty) = deps.first() {
			if !ty.derive.iter().any(|d| d == "Default") {
				// Ensure map contains this value which is definitively false.
				default_types.insert(typ, false);
				continue;
			}
		}

		// Type is default if every dependency also has default.
		let has_no_default = deps.iter().any(|dep| {
			if let Some(v) = default_types.get(dep.name.as_ref()) {
				*v == false
			} else {
				dep.derive.iter().any(|d| d == "Default") == false
			}
		});

		// Cache result
		default_types.insert(typ, !has_no_default);
	}

	// Go through all types that do not (or should not) be eligible and update.
	let to_update: Vec<String> = default_types
		.into_iter()
		.filter(|(_, v)| *v == false)
		.map(|(t, _)| t.to_owned())
		.collect();
	for ty in to_update {
		let rtype = cr8.get_type_mut(&ty).unwrap();
		if let Some(pos) = rtype.derive.iter().position(|d| *d == "Default") {
			rtype.derive.remove(pos);
		}
	}
}

/// Implements parent traits then flattens "parent" fields into child.
fn flatten_extends(cr8: &mut Crate, mod_path: &Path) {
	let Some(module) = cr8.contents.get_mut(mod_path) else {
		return;
	};

	let types_with_extends: Vec<_> = module
		.types
		.values()
		.filter_map(|t| match t.extends {
			Some(_) if t.output => Some(t.name.clone()),
			_ => None,
		})
		.collect();

	// First iterate to implement the traits but don't modify fields to avoid
	// polluting field and therefore trait definitions.
	let mut type_stack = Vec::new();
	for rname in types_with_extends.iter() {
		// Iterate extends until we reach the root type.
		let mut cur = rname;
		while let Some(ty) = cr8.get_type(&cur) {
			if let Some(ref etype) = ty.extends {
				type_stack.push(etype.clone());
				cur = type_stack.last().unwrap();
			} else {
				break;
			}
		}

		// While stack has item, add its fields and implement its mirror trait.
		let mut trait_impls = Vec::new();
		let mut parent_empty_fields = true;
		while let Some(tname) = type_stack.pop() {
			let ty = cr8.get_type(&tname).unwrap();
			match ty.members {
				TypeMembers::Fields(ref fields) => {
					// Construct full trait path
					let trait_name = Trait::trait_name_for(&tname);
					let mut trait_path = cr8.get_path_for_trait(&trait_name).unwrap();
					_ = write!(trait_path, "::{trait_name}");

					// Make trait impl block
					let mut trait_impl = format!("impl {trait_path} for {rname} {{\n",);
					for field in fields {
						if let Some(s) = impl_mirror_trait_fn(&cr8, &field.name, &field) {
							trait_impl.push_str(&s);
						}
					}
					trait_impl.push_str("}\n");

					trait_impls.push(trait_impl);

					if fields.is_empty() {
						parent_empty_fields = true;
					}
				}
				TypeMembers::Variants(_) if !parent_empty_fields => panic!("Invalid"),
				TypeMembers::Variants(_) => parent_empty_fields = false,
				_ => panic!("Non-struct types cannot extend another."),
			};
		}

		// Update type with trait impls.
		let rtype = cr8.get_type_mut(&rname).unwrap();
		rtype.impls.extend_from_slice(&trait_impls);
	}

	// Then iterate again to actually replace the fields
	let mut rname_extends = Vec::new();
	for rname in types_with_extends.iter() {
		// Iterate extends until we reach the root type.
		let mut cur = rname;
		while let Some(ty) = cr8.get_type(&cur) {
			if let Some(ref etype) = ty.extends {
				type_stack.push(etype.clone());
				cur = type_stack.last().unwrap();
			} else {
				break;
			}
		}

		// While stack has item, add its fields and implement its mirror trait.
		let mut to_add = Vec::new();
		while let Some(tname) = type_stack.pop() {
			let ty = cr8.get_type(&tname).unwrap();
			match ty.members {
				TypeMembers::Fields(ref fields) => to_add.extend_from_slice(&fields),
				TypeMembers::Variants(_) => {}
				_ => panic!("Non-struct types cannot extend another."),
			};
		}

		// Update type with parent fields and trait impls
		let rtype = cr8.get_type_mut(&rname).unwrap();
		let parent = rtype.extends.take().unwrap();
		match rtype.members {
			TypeMembers::Fields(ref mut fields) => {
				// Parent fields go first, existing fields afterwards.
				to_add.extend(fields.drain(..));
				*fields = to_add;
			}
			TypeMembers::Variants(_) => {}
			_ => panic!("Non-struct types cannot extend another."),
		}

		rname_extends.push((rname, parent));
	}

	// For every type that extended an abstract type, convert to a choice enum.
	for (rname, parent) in rname_extends.iter() {
		let rtype = cr8.get_type_mut(&rname).unwrap();

		// If parent is abstract, convert to choice type (if necessary) then add variant.
		let rtype_field = Field {
			name: String::new(),
			typename: rtype.name.to_string(),
			attrs: Vec::new(),
			rename: None,
			docs: String::new(),
			public: true,
			plurality: FieldPlurality::None,
			boxed: true,
		};
		let rtype_variant = Variant {
			name: rtype.name.clone(),
			docs: String::new(),
			rename: rtype.rename.clone(),
			attrs: Vec::new(),
			vtype: VariantType::Newtype(rtype_field),
		};

		let ptype = cr8.get_type_mut(&parent).unwrap();
		if !ptype.output {
			match ptype.members {
				TypeMembers::Fields(_) => {
					ptype.attrs.clear();
					ptype.derive = CHOICE_DERIVES.iter().map(|&v| v.to_owned()).collect();
					ptype.members = TypeMembers::Variants(vec![rtype_variant])
				}
				TypeMembers::Variants(ref mut variants) => variants.push(rtype_variant),
				_ => {}
			};
		}
	}
}

/// Ensure all abstract types are variants or unit structs, and add serde impl.
pub fn update_abstract_types(cr8: &mut Crate, src_path: &Path) {
	let Some(src_mod) = cr8.contents.get_mut(src_path) else {
		return;
	};

	// Find the abstract types
	let abstract_types: Vec<Rc<str>> = src_mod
		.types
		.values()
		.filter(|r| !r.output)
		.map(|r| r.name.clone())
		.collect();

	for rname in abstract_types {
		// If this type is not actually used in schema, keep it here as unit struct.
		let rtype = src_mod.types.get_mut(&rname).unwrap();
		rtype.output = true;
		if let TypeMembers::Fields(ref mut fields) = rtype.members {
			// Any abstracts that hit this are not used, so should be unit structs.
			fields.clear();
			continue;
		};

		// Update output and add appropriate impl for serde.
		match rtype.members {
			// Variants are what we want for abstract types.
			TypeMembers::Variants(ref mut variants) => {
				let mut serde_macro_impl = format!("struct_like_serde! {{\n\t{}\n", rtype.name);
				for variant in variants {
					let rename = variant.rename.as_ref().unwrap_or(&variant.name);
					_ = write!(serde_macro_impl, "\t{} -> \"{}\",\n", variant.name, rename);

					// TODO: Consider moving this elsewhere or adding some indication of
					//       whether the rename should always output the serde tag.
					variant.rename = None;
				}
				_ = write!(serde_macro_impl, "}}");
				rtype.impls.push(serde_macro_impl);
			}
			// Any abstracts that hit this are not used, so should be unit structs.
			TypeMembers::Fields(_) => unreachable!(),
			TypeMembers::Alias(_) => panic!("Alias not expected when processing abstract types."),
		};
	}
}

fn main() {
	let mut args = env::args();
	if args.len() < 2 {
		eprintln!("Usage: xsd_generator [-n crate_name] [-o output_dir] XSD1 [XSD2] ...");
		return;
	}

	let mut output_dir: PathBuf = Path::new(".").into();
	let mut crate_name = None;
	let mut xsd_files = Vec::new();
	_ = args.next(); // Skip first arg.
	while let Some(arg) = args.next() {
		match arg {
			// Parse options
			opt if opt.starts_with('-') => {
				match opt.as_str() {
					"-n" => {
						if let Some(next_arg) = args.next() {
							crate_name = Some(next_arg);
						} else {
							eprintln!("'-o' expects an argument");
							return;
						}
					}
					"-o" => {
						if let Some(next_arg) = args.next() {
							output_dir = PathBuf::from(next_arg);
						} else {
							eprintln!("'-o' expects an argument");
							return;
						}
					}
					_ => {
						eprintln!("Unrecognized option '{opt}'.");
						return;
					}
				};
			}
			_ => xsd_files.push(arg),
		};
	}

	let mut elements = Vec::new();
	for file in xsd_files.iter() {
		if let Err(e) = parse_schema(file, |e| elements.push(e)) {
			eprintln!("Parsing schema from '{file}' failed: {e}");
			return;
		}
	}
	println!("Parsed {} elements.", elements.len());

	// Get and remove all schemas
	let mut schemas = Vec::new();
	let mut i = 0;
	while i < elements.len() {
		if *elements[i].type_.as_ref().unwrap() == ContentType::Schema {
			schemas.push(elements.swap_remove(i));
			continue;
		}

		i += 1;
	}
	println!("Schemas:");
	for sch in schemas.iter() {
		println!("\t{}", sch.name);
	}

	println!("Schema-level elements:");
	for schema in schemas.iter() {
		let mut ns = "";
		if let Some(target) = schema.attrs.get("targetNamespace") {
			for (key, val) in schema.attrs.iter() {
				if val == target {
					ns = key.trim_start_matches("xmlns:");
				}
			}
		}

		println!("Namespace({ns}) - {}", schema.name);
		println!("\t{} top-level xs:element.", schema.values.len());
		elements.extend_from_slice(&schema.values);
	}

	// Sort remaining alphabetically
	elements.sort_by(|e1, e2| e1.name.cmp(&e2.name));
	for i in 1..elements.len() {
		if elements[i].name == elements[i - 1].name {
			eprintln!("ERROR: Duplicate name {}", &elements[i].name);
		}
	}

	let mut alphabet_count = [0; 26];
	for e in elements.iter().skip(1) {
		let ch = e.name.chars().next().unwrap();
		if let c @ 'a'..='z' = ch.to_ascii_lowercase() {
			alphabet_count[c as usize - 'a' as usize] += 1;
		}
	}
	println!("Type name alphabet count:");
	for (count, letter) in alphabet_count.iter().zip('A'..='Z') {
		println!("{letter}: {count}");
	}

	// If a crate name was not given by the user, try to derive a name from the
	// first schema given.
	if crate_name.is_none() {
		if let Some(target) = schemas[0].attrs.get("targetNamespace") {
			for (key, val) in schemas[0].attrs.iter() {
				if val == target {
					crate_name = Some(key.trim_start_matches("xmlns:").to_string());
				}
			}
		}
	}
	let name = crate_name.unwrap_or(String::from("messages"));

	// Generate crate contents.
	let mut cr8 = Crate::new(&name);
	cr8.deps
		.push(r#"serde = { version = "1.0", features = ["derive"] }"#.into());
	cr8.deps
		.push(r#"uuid = { version = "1.23", features = ["serde"] }"#.into());
	cr8.deps
		.push(r#"chrono = { version = "0.4.45", features = ["serde"] }"#.into());
	cr8.deps.push(r#"paste = "1.0""#.into());

	strip_xsd_ns(&mut elements);

	add_schema(&schemas[0], &mut cr8);
	add_schema_elements(&elements, &mut cr8);
	add_choices(&elements, &mut cr8);
	add_enum_types(&elements, &mut cr8);
	add_complex_structs(&elements, &mut cr8);
	add_simple_structs(&elements, &mut cr8);

	// MUST GO BEFORE FLATTEN
	add_mirror_traits(&mut cr8, Path::new("crate/types"));

	// This changes the message fields and removes `extends`.
	// MUST GO BEFORE DEFAULTS.
	flatten_extends(&mut cr8, Path::new("crate/types"));
	flatten_extends(&mut cr8, Path::new("crate/choices"));

	update_abstract_types(&mut cr8, Path::new("crate/types"));

	// This MUST go before updating type paths since lookups are via pathless
	// names.
	correct_defaults(&mut cr8);

	contents::serde_utils::add_utils(&mut cr8);

	cr8.update_type_paths();

	impls_for_types(&mut cr8);

	// Create crate
	cr8.output(&output_dir).expect("Crate creation failed");
}
