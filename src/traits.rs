//! Utilities for creation and implementation of traits.

use crate::contents::{Crate, Field, FieldPlurality, Module, RustType, TypeMembers};
use std::{
	fmt::{self, Write as _},
	path::{Path, PathBuf},
	rc::Rc,
};

#[derive(Debug)]
pub struct AssociatedType {
	pub name: String,
	pub bounds: Vec<Rc<str>>,
	pub default_type: Option<String>,
}
#[derive(Debug)]
pub struct Func {
	pub name: String,
	/// Function parameters including parenthesis and generics.
	pub params: String,
	pub ret: String,
	/// Function body without opening or closing braces.
	pub contents: String,
}
#[derive(Debug)]
pub struct Trait {
	pub name: Rc<str>,
	pub supertraits: Vec<Rc<str>>,
	pub associated_types: Vec<AssociatedType>,
	pub fns: Vec<Func>,
}
impl Trait {
	pub fn new(name: &str) -> Self {
		Trait {
			name: Rc::from(name),
			supertraits: Vec::new(),
			associated_types: Vec::new(),
			fns: Vec::new(),
		}
	}

	/// Return a string with the expected/assumed trait name of the given type.
	pub fn trait_name_for(typename: &str) -> String {
		// For now, trait name identical to type name
		typename.to_owned()
	}
}
impl fmt::Display for Trait {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "pub trait {}", self.name)?;

		let supertraits = self.supertraits.join("\n\t+ ");
		if !supertraits.is_empty() {
			write!(f, ":\n\t{supertraits}\n{{\n")?;
		} else {
			write!(f, " {{\n")?;
		}

		// Associated types go at the top
		for ty in self.associated_types.iter() {
			let bounds = ty.bounds.join(" + ");
			write!(f, "\ttype {}: {}", ty.name, bounds)?;

			if let Some(ref def) = ty.default_type {
				write!(f, " = {def};\n")?;
			} else {
				write!(f, ";\n")?;
			}
		}

		// Now output functions
		for fun in self.fns.iter() {
			write!(f, "\tfn {}{}", fun.name, fun.params)?;

			// If return type, write that.
			if !fun.ret.is_empty() {
				write!(f, " -> {}", fun.ret)?;
			}

			// If there are contents, output block. Otherwise semicolon.
			if !fun.contents.is_empty() {
				write!(f, " {{\n{}}}", fun.contents)?;
			} else {
				write!(f, ";\n")?;
			}
		}

		write!(f, "}}\n")
	}
}

/// Add a trait to `cr8` that mirrors the contents of `rtype`.
pub fn create_mirror_trait(cr8: &Crate, rtype: &RustType) -> Option<Trait> {
	let mut t = Trait::new(&rtype.name);
	let TypeMembers::Fields(ref fields) = rtype.members else {
		return None;
	};

	// If this type extends another, this trait should use that type's trait as a
	// supertrait. Otherwise use the type's derives.
	if let Some(ref p) = rtype.extends {
		t.supertraits
			.push(Trait::trait_name_for(&p).as_str().into());
	} else {
		static ABSTRACT_DERIVES: &[&str] = &[
			"Clone",
			"core::fmt::Debug",
			"for<'a> serde::Deserialize<'a>",
			"PartialEq",
			"serde::Serialize",
		];

		t.supertraits
			.extend(ABSTRACT_DERIVES.iter().map(|&d| Rc::from(d)));
	}

	// For each mandatory field:
	// - `get_FIELD(&self) -> &FIELDTYPE`
	// - `get_FIELD_mut(&mut self) -> &mut FIELDTYPE`
	// For each optional field:
	// - `has_FIELD(&self) -> bool`
	// - `get_FIELD(&self) -> Option<&FIELDTYPE>`
	// - `get_FIELD_mut(&mut self) -> Option<&mut FIELDTYPE>`
	// For plural fields:
	// - `get_FIELD(&self) -> &Vec<FIELDTYPE>`
	// - `get_FIELD_mut(&mut self) -> &mut Vec<FIELDTYPE>`
	for field in fields.iter() {
		// Need to use full path to type to avoid imports.
		let full_path = match cr8.get_path_for_type(&field.typename) {
			Some(mut p) => {
				_ = write!(p, "::{}", field.typename);
				p
			}
			None => field.typename.clone(),
		};

		// Remove underscore from end of keyword field names to avoid duplicate.
		let fn_name_type = field.name.trim_end_matches('_');
		match field.plurality {
			FieldPlurality::None => {
				t.fns.push(Func {
					name: format!("get_{fn_name_type}"),
					params: "(&self)".to_string(),
					ret: format!("&{full_path}"),
					contents: String::new(),
				});
				t.fns.push(Func {
					name: format!("get_{fn_name_type}_mut"),
					params: "(&mut self)".to_string(),
					ret: format!("&mut {full_path}"),
					contents: String::new(),
				});
			}
			FieldPlurality::Optional => {
				t.fns.push(Func {
					name: format!("has_{fn_name_type}"),
					params: "(&self)".to_string(),
					ret: "bool".to_owned(),
					contents: String::new(),
				});
				t.fns.push(Func {
					name: format!("get_{fn_name_type}"),
					params: "(&self)".to_string(),
					ret: format!("Option<&{full_path}>"),
					contents: String::new(),
				});
				t.fns.push(Func {
					name: format!("get_{fn_name_type}_mut"),
					params: "(&mut self)".to_string(),
					ret: format!("Option<&mut {full_path}>"),
					contents: String::new(),
				});
			}
			FieldPlurality::Plural => {
				t.fns.push(Func {
					name: format!("get_{fn_name_type}"),
					params: "(&self)".to_string(),
					ret: format!("&Vec<{full_path}>"),
					contents: String::new(),
				});
				t.fns.push(Func {
					name: format!("get_{fn_name_type}_mut"),
					params: "(&mut self)".to_string(),
					ret: format!("&mut Vec<{full_path}>"),
					contents: String::new(),
				});
			}
			FieldPlurality::OptionalPlural => {
				t.fns.push(Func {
					name: format!("has_{fn_name_type}"),
					params: "(&self)".to_string(),
					ret: "bool".to_owned(),
					contents: String::new(),
				});
				t.fns.push(Func {
					name: format!("get_{fn_name_type}"),
					params: "(&self)".to_string(),
					ret: format!("Option<&Vec<{full_path}>>"),
					contents: String::new(),
				});
				t.fns.push(Func {
					name: format!("get_{fn_name_type}_mut"),
					params: "(&mut self)".to_string(),
					ret: format!("Option<&mut Vec<{full_path}>>"),
					contents: String::new(),
				});
			}
		};
	}

	Some(t)
}

/// Add all mirror traits to [Crate].
pub fn add_mirror_traits(cr8: &mut Crate, mod_path: &Path) {
	let Some(module) = cr8.contents.get(mod_path) else {
		return;
	};

	// Create module
	let mut trait_mod = Module::default();
	_ = write!(trait_mod.docs, "Module for all traits.\n\n");

	for rtype in module.types.values() {
		if let Some(t) = create_mirror_trait(cr8, rtype) {
			trait_mod.traits.insert(rtype.name.clone(), t);
		}
	}

	// Export module in library.
	let lib = cr8.contents.entry(PathBuf::from("crate")).or_default();
	lib.modules.insert(String::from("pub mod traits;"));

	// Create module for contents.
	let crate_path = PathBuf::from("crate/traits");
	cr8.contents.insert(crate_path, trait_mod);
}

/// Uses `t_fieldname` to derive the function name, and `field` for contents.
pub fn impl_mirror_trait_fn(cr8: &Crate, t_fieldname: &str, field: &Field) -> Option<String> {
	let fn_name_type = t_fieldname.trim_end_matches('_');
	let type_path = match cr8.get_path_for_type(&field.typename) {
		Some(p) => format!("{p}::{}", field.typename),
		None => field.typename.clone(),
	};

	let mut fns = String::new();
	match field.plurality {
		FieldPlurality::None => {
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}(&self) -> &{} {{ &self.{} }}\n",
				type_path, field.name
			);
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}_mut(&mut self) -> &mut {} {{ &mut self.{} }}\n",
				type_path, field.name
			);
		}
		FieldPlurality::Optional => {
			_ = write!(
				fns,
				"\tfn has_{fn_name_type}(&self) -> bool {{ self.{}.is_some() }}\n",
				field.name
			);
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}(&self) -> Option<&{}> {{ self.{}.as_ref() }}\n",
				type_path, field.name
			);
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}_mut(&mut self) -> Option<&mut {}> {{ self.{}.as_mut() }}\n",
				type_path, field.name
			);
		}
		FieldPlurality::Plural => {
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}(&self) -> &Vec<{}> {{ &self.{} }}\n",
				type_path, field.name
			);
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}_mut(&mut self) -> &mut Vec<{}> {{ &mut self.{} }}\n",
				type_path, field.name
			);
		}
		FieldPlurality::OptionalPlural => {
			_ = write!(
				fns,
				"\tfn has_{fn_name_type}(&self) -> bool {{ self.{}.is_some() }}\n",
				field.name
			);
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}(&self) -> Option<&Vec<{}>> {{ self.{}.as_ref() }}\n",
				type_path, field.name
			);
			_ = write!(
				fns,
				"\tfn get_{fn_name_type}_mut(&mut self) -> Option<&mut Vec<{}>> {{ self.{}.as_mut() }}\n",
				type_path, field.name
			);
		}
	};

	// Don't return an empty string
	if !fns.is_empty() { Some(fns) } else { None }
}
