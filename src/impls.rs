use crate::{
	contents::{Crate, RustType},
	names::to_uppercase_underscore,
};
use std::{
	collections::{BTreeMap, BTreeSet},
	fmt::Write,
};

/// Implement convenience conversions and such for all newtypes.
pub fn impls_for_types(cr8: &mut Crate) {
	for module in cr8.contents.values_mut() {
		for rtype in module.types.values_mut() {
			// Newtype-specific impls
			if let Some(f) = rtype.get_newtype_field() {
				let parent = f.typename.to_owned();

				impl_derefs(&mut module.imports, rtype, &parent);
				impl_as_refmut(&mut module.imports, rtype, &parent);
				impl_from_into(&mut module.imports, rtype, &parent);
			}
		}
	}
}

/// Implement [Deref] and [DerefMut] for types that extend another type.
pub fn impl_derefs(imports: &mut BTreeSet<String>, rtype: &mut RustType, parent: &str) {
	let mut imp = String::new();

	imports.insert("use std::ops::{Deref, DerefMut};".to_string());

	// Normal
	_ = write!(imp, "impl Deref for {} {{\n", rtype.name);
	_ = write!(imp, "\ttype Target = {};\n", parent);
	imp.push_str("\tfn deref(&self) -> &Self::Target {\n");

	// If parent field is only field, newtype.
	if rtype.is_newtype_struct() {
		imp.push_str("\t\t&self.0\n");
	} else {
		imp.push_str("\t\t&self._base\n");
	}

	imp.push_str("\t}\n}");

	// Push Deref
	rtype.impls.push(imp);

	// Mut
	let mut imp = String::new();
	_ = write!(imp, "impl DerefMut for {} {{\n", rtype.name);
	imp.push_str("\tfn deref_mut(&mut self) -> &mut Self::Target {\n");

	// If parent field is only field, newtype.
	if rtype.is_newtype_struct() {
		imp.push_str("\t\t&mut self.0\n");
	} else {
		imp.push_str("\t\t&mut self._base\n");
	}

	imp.push_str("\t}\n}");

	// Push DerefMut
	rtype.impls.push(imp);
}

/// Implement [Deref] and [DerefMut] for types that extend another type.
pub fn impl_as_refmut(imports: &mut BTreeSet<String>, rtype: &mut RustType, parent: &str) {
	let mut imp = String::new();

	imports.insert("use std::convert::{AsRef, AsMut};".to_owned());

	// Normal
	_ = write!(imp, "impl AsRef<{parent}> for {} {{\n", rtype.name);
	_ = write!(imp, "\tfn as_ref(&self) -> &{parent} {{\n");

	// If parent field is only field, newtype.
	if rtype.is_newtype_struct() {
		imp.push_str("\t\t&self.0\n");
	} else {
		imp.push_str("\t\t&self._base\n");
	}

	imp.push_str("\t}\n}");

	// Push AsRef
	rtype.impls.push(imp);

	// Mut
	let mut imp = String::new();
	_ = write!(imp, "impl AsMut<{parent}> for {} {{\n", rtype.name);
	_ = write!(imp, "\tfn as_mut(&mut self) -> &mut {parent} {{\n");

	// If parent field is only field, newtype.
	if rtype.is_newtype_struct() {
		imp.push_str("\t\t&mut self.0\n");
	} else {
		imp.push_str("\t\t&mut self._base\n");
	}

	imp.push_str("\t}\n}");

	// Push AsMut
	rtype.impls.push(imp);
}

/// Implement [From] and [Into] for child types.
pub fn impl_from_into(imports: &mut BTreeSet<String>, rtype: &mut RustType, parent: &str) {
	imports.insert("use std::convert::{From, Into};".to_owned());

	// Can only implement [From] when parent is sole member.
	if rtype.is_newtype_struct() {
		let mut imp = String::new();
		_ = write!(imp, "impl From<{parent}> for {} {{\n", rtype.name);
		_ = write!(imp, "\tfn from(p: {parent}) -> Self {{\n");

		// If parent field is only field, newtype.
		if rtype.is_newtype_struct() {
			imp.push_str("\t\tSelf(p)\n");
		} else {
			imp.push_str("\t\tSelf { _base: p }\n");
		}

		imp.push_str("\t}\n}");

		// Push [From]
		rtype.impls.push(imp);
	}

	// Into
	let mut imp = String::new();
	_ = write!(imp, "impl Into<{parent}> for {} {{\n", rtype.name);
	_ = write!(imp, "\tfn into(self) -> {parent} {{\n");

	// If parent field is only field, newtype.
	if rtype.is_newtype_struct() {
		imp.push_str("\t\tself.0\n");
	} else {
		imp.push_str("\t\tself._base\n");
	}

	imp.push_str("\t}\n}\n");

	// Push [Into]
	rtype.impls.push(imp);
}

/// Add associated constants for every attribute on this type in the schema.
pub fn impl_attrs(rtype: &mut RustType, attrs: &BTreeMap<String, String>) {
	if attrs.is_empty() {
		return;
	}

	// Make impl block for constants
	let mut attr_impl_block = format!(
		"/// Block for all attribute constants\nimpl {} {{\n",
		rtype.name
	);
	for (name, value) in attrs.iter() {
		let attr_name = &to_uppercase_underscore(name).unwrap_or(name.clone());
		_ = write!(
			attr_impl_block,
			"\tconst {attr_name}: &'static str = r#\"{value}\"#;\n"
		);
	}
	attr_impl_block.push_str("}");

	rtype.impls.push(attr_impl_block);
}
