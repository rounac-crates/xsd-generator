//! Contents of a crate
//!

pub mod serde_utils;

use crate::traits::Trait;
use std::{
	collections::{BTreeMap, BTreeSet, HashMap, hash_map},
	fmt::{self, Display, Write as _},
	fs,
	io::{self, Write as _},
	path::{Path, PathBuf},
	rc::Rc,
};

pub struct Crate {
	pub name: String,
	/// HashMap<Path, String> where path is the namespace but with slashes, so
	/// `crate/mod1/mod2` --> `crate::mod1::mod2`, and the string is the file
	/// contents.
	pub contents: HashMap<PathBuf, Module>,
	/// Map of typename --> crate location. See `contents` for location format.
	pub type_map: HashMap<Rc<str>, PathBuf>,
	/// Valid dependency string for `Cargo.toml` without trailing newline.
	pub deps: Vec<String>,
	/// Valid feature string for `Cargo.toml` without trailing newline.
	pub features: Vec<String>,
}
impl Crate {
	pub fn new(name: &str) -> Self {
		Crate {
			name: name.into(),
			contents: HashMap::new(),
			type_map: HashMap::new(),
			deps: Vec::new(),
			features: Vec::new(),
		}
	}

	pub fn get_deps<'a>(
		&'a self,
		name: &str,
		non_optional_only: bool,
	) -> Option<Vec<&'a RustType>> {
		let starting_type = self.get_type(name)?;
		// Map of visited nodes. 1 - currently visiting, 2 - fully visited.
		// If visiting a dep and this map contains a 1, there is a dep cycle.
		let mut visited: HashMap<Rc<str>, u8> = HashMap::new();
		// Stack of type and index of next field/variant to visit
		let mut travel_stack: Vec<(&RustType, usize)> = vec![(starting_type, 0)];
		let mut deps = vec![starting_type];

		while let Some((ty, idx)) = travel_stack.last_mut() {
			// Currently visiting this node.
			let mut ticket = match visited.entry(ty.name.clone()) {
				hash_map::Entry::Vacant(v) => {
					deps.push(ty);

					v.insert_entry(1)
				}
				hash_map::Entry::Occupied(o) => {
					// If this stack entry was newly placed yet still visited, then there is
					// a cycle.
					if *idx == 0 && *o.get() == 1 {
						eprintln!("Crate::get_deps encountered cycle in {}.", ty.name);
						travel_stack.pop();
						continue;
					} else if *o.get() == 2 {
						travel_stack.pop();
						continue;
					} else {
						o
					}
				}
			};

			// Save and update current before we invalidate these references
			let cur_idx = *idx;
			*idx += 1;

			match ty.members {
				TypeMembers::Alias(ref s) => {
					if let Some(t) = self.get_type(s)
						&& cur_idx == 0
					{
						travel_stack.push((t, 0));
					} else {
						// Mark this type as fully visited
						*ticket.get_mut() = 2;

						// Pop to continue.
						travel_stack.pop();
					}
				}
				TypeMembers::Fields(ref fields) => {
					// If there is a field, try to push its type onto stack.
					// Else no more fields, pop stack.
					if let Some(field) = fields.get(cur_idx) {
						// Ensure we only travel to the desired deps.
						let should_insert =
							!non_optional_only || field.plurality == FieldPlurality::None;
						if let Some(t) = self.get_type(&field.typename)
							&& should_insert
						{
							travel_stack.push((t, 0));
						}
					} else {
						// Mark this type as fully visited
						*ticket.get_mut() = 2;

						// Pop to continue.
						travel_stack.pop();
					}
				}
				TypeMembers::Variants(ref variants) => {
					// If there is a variant, try to push all types onto stack.
					// Else no more variants, pop stack.
					if let Some(variant) = variants.get(cur_idx) {
						match variant.vtype {
							VariantType::Newtype(ref field) => {
								let should_insert =
									!non_optional_only || field.plurality == FieldPlurality::None;
								if let Some(t) = self.get_type(&field.typename)
									&& should_insert
								{
									travel_stack.push((t, 0));
								}
							}
							VariantType::Tuple(ref fields) => {
								for field in fields {
									let should_insert = !non_optional_only
										|| field.plurality == FieldPlurality::None;
									if let Some(t) = self.get_type(&field.typename)
										&& should_insert
									{
										travel_stack.push((t, 0));
									}
								}
							}
							_ => {}
						};
					} else {
						// Mark this type as fully visited
						*ticket.get_mut() = 2;

						// Pop to continue.
						travel_stack.pop();
					}
				}
			};
		}

		Some(deps)
	}

	pub fn get_type(&self, name: &str) -> Option<&RustType> {
		let p = self.type_map.get(name)?;
		self.contents.get(p).and_then(|m| m.types.get(name))
	}

	pub fn get_type_mut(&mut self, name: &str) -> Option<&mut RustType> {
		let p = self.type_map.get(name);
		self.contents
			.get_mut(p.unwrap())
			.and_then(|m| m.types.get_mut(name))
	}

	/// Create this crate in `dir`.
	pub fn output<P: AsRef<Path>>(&self, dir: P) -> io::Result<()> {
		let crate_path = dir.as_ref();
		_ = std::fs::remove_dir_all(crate_path); // Delete folder always.
		let src_path = crate_path.join("src");

		// Create base crate folder first using cargo.
		println!(
			"Creating crate \"{}\" with {} features in {crate_path:?}..",
			self.name,
			self.features.len()
		);
		let mut cargo_res = std::process::Command::new("cargo")
			.arg("new")
			.arg("--name")
			.arg(&self.name)
			.arg("--lib")
			.arg(crate_path)
			.stdin(std::process::Stdio::null())
			.stdout(std::process::Stdio::null())
			.stderr(std::process::Stdio::null())
			.spawn()?;

		let exit = cargo_res.wait()?;
		if !exit.success() {
			return Err(io::ErrorKind::Other.into());
		}

		// Add deps to crate
		let manifest_path = crate_path.join("Cargo.toml");
		if let Ok(mut manifest) = fs::OpenOptions::new()
			.append(true)
			.create(false)
			.open(manifest_path)
		{
			// Write deps immediately (they go under `dependencies` table)
			for dep in self.deps.iter() {
				write!(manifest, "{}\n", dep)?;
			}

			// Write features afterwards.
			write!(manifest, "\n[features]\n")?;
			for feat in self.features.iter() {
				write!(manifest, "{}\n", feat)?;
			}
		}

		// Output root content
		let lib_path = src_path.join("lib.rs");
		let lib_file = &mut fs::File::create(lib_path)?;
		if let Some(contents) = self.contents.get(Path::new("crate")) {
			let mut capitalized = self.name.clone();
			if let Some(first) = capitalized.chars().next() {
				let upper: String = first.to_uppercase().collect();
				capitalized = capitalized.replacen(first, &upper, 1);
			}
			let first_line = format!("//! {}\n//!\n", capitalized);
			lib_file.write(first_line.as_bytes())?; // Ensure crate name always first.

			_ = write!(lib_file, "{contents}")?;
		}

		// Create files for remaining keys
		let non_root_entries = self
			.contents
			.iter()
			.filter(|(k, _)| *k != Path::new("crate"));
		for (module, contents) in non_root_entries {
			println!("Creating module {}", module.to_str().unwrap());

			// Create the module directory
			let mod_rel_path = module.strip_prefix("crate/").unwrap();
			let mod_path = src_path.join(mod_rel_path);
			if let Some(mod_parents) = mod_rel_path.ancestors().skip(1).next() {
				let mod_dir = src_path.join(mod_parents);
				fs::DirBuilder::new().recursive(true).create(mod_dir)?;
			}

			// Create the module file and add contents
			let mod_path = mod_path.with_extension("rs");
			let mod_file = &mut fs::File::create(mod_path)?;
			//mod_file.write(contents.as_bytes())?;
			_ = write!(mod_file, "{contents}");
		}

		Ok(())
	}

	/// Go through all fields and variants to update type names with full mod path.
	pub fn update_type_paths(&mut self) {
		for (mod_path, module) in self.contents.iter_mut() {
			for rtype in module.types.values_mut() {
				match rtype.members {
					TypeMembers::Alias(ref mut ty) => {
						if let Some(type_path) = self.type_map.get(ty.as_str()) {
							if type_path != mod_path {
								let mut crate_path = type_path.to_str().unwrap().replace("/", "::");
								crate_path.push_str("::");
								ty.insert_str(0, &crate_path);
							}
						}
					}
					TypeMembers::Fields(ref mut fields) => {
						for field in fields {
							// If type is not found, it's builtin or not found.
							if let Some(type_path) = self.type_map.get(field.typename.as_str()) {
								if type_path != mod_path {
									let mut crate_path =
										type_path.to_str().unwrap().replace("/", "::");
									crate_path.push_str("::");
									field.typename.insert_str(0, &crate_path);
								}
							}
						}
					}
					TypeMembers::Variants(ref mut variants) => {
						for variant in variants {
							match variant.vtype {
								VariantType::Newtype(ref mut f) => {
									// If type is not found, it's builtin or not found.
									if let Some(type_path) = self.type_map.get(f.typename.as_str())
									{
										if type_path != mod_path {
											let mut crate_path =
												type_path.to_str().unwrap().replace("/", "::");
											crate_path.push_str("::");
											f.typename.insert_str(0, &crate_path);
										}
									}
								}
								_ => {}
							};
						}
					}
				};
			}
		}
	}

	/// Return a valid path to module containing `typename`. Typename not included.
	pub fn get_path_for_type(&self, typename: &str) -> Option<String> {
		self.type_map
			.get(typename)
			.map(|t| t.to_str().unwrap().replace("/", "::"))
	}

	/// Return a valid path to module containing `typename`. Typename not included.
	pub fn get_path_for_trait(&self, trait_name: &str) -> Option<String> {
		for (mod_path, module) in self.contents.iter() {
			if module.traits.get(trait_name).is_some() {
				return Some(mod_path.to_str().unwrap().replace("/", "::"));
			}
		}

		None
	}
}

#[derive(Debug, Default)]
pub struct Module {
	/// Documentation without any additional syntax.
	pub docs: String,
	/// Full import strings including semicolon but without newline.
	pub imports: BTreeSet<String>,
	/// Full module declaration strings including semicolon but without newline.
	pub modules: BTreeSet<String>,
	pub types: BTreeMap<Rc<str>, RustType>,
	pub traits: BTreeMap<Rc<str>, Trait>,
	/// Full function definition string without newline.
	pub fns: Vec<String>,
}
impl Display for Module {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		_ = write!(f, "#![doc = r#\"{}\"#]\n\n", self.docs.trim())?;

		// Output modules (if any)
		if self.modules.len() > 0 {
			for module in self.modules.iter() {
				_ = write!(f, "{}\n", module)?;
			}

			f.write_char('\n')?;
		}

		// Output imports (if any)
		if self.imports.len() > 0 {
			for import in self.imports.iter() {
				_ = write!(f, "{}\n", import)?;
			}

			f.write_char('\n')?;
		}

		// Output functions
		for fun in self.fns.iter() {
			_ = write!(f, "{fun}\n")?;
		}

		// Output traits
		for t in self.traits.values() {
			_ = write!(f, "{t}\n")?;
		}

		// Output all types
		for t in self.types.values() {
			_ = write!(f, "{}\n", t)?;
		}

		Ok(())
	}
}

/// Whether a field is normal, optional, or repeated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldPlurality {
	/// Type is simply type name.
	None,
	/// Type should be [Option].
	Optional,
	/// Type should be [Vec] or equivalent.
	Plural,
	/// Type should be [Vec] of [Option].
	OptionalPlural,
}

#[derive(Clone, Debug)]
pub struct Field {
	pub name: String,
	pub typename: String,
	pub public: bool,
	pub plurality: FieldPlurality,
	pub boxed: bool,
	pub docs: String,
	pub rename: Option<Rc<str>>,
	pub attrs: Vec<String>,
}
impl Field {
	pub fn get_full_type(&self) -> String {
		match self.plurality {
			FieldPlurality::None if self.boxed => format!("Box<{}>", self.typename),
			FieldPlurality::None => format!("{}", self.typename),
			FieldPlurality::Optional if self.boxed => {
				format!("Option<Box<{}>>", self.typename)
			}
			FieldPlurality::Optional => format!("Option<{}>", self.typename),
			FieldPlurality::Plural if self.boxed => format!("Vec<Box<{}>>", self.typename),
			FieldPlurality::Plural => format!("Vec<{}>", self.typename),
			FieldPlurality::OptionalPlural if self.boxed => {
				format!("Option<Vec<Box<{}>>>", self.typename)
			}
			FieldPlurality::OptionalPlural => format!("Option<Vec<{}>>", self.typename),
		}
	}
}
impl Display for Field {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Prepare full type
		let full_type = match self.plurality {
			FieldPlurality::None if self.boxed => format_args!("Box<{}>", self.typename),
			FieldPlurality::None => format_args!("{}", self.typename),
			FieldPlurality::Optional if self.boxed => {
				format_args!("Option<Box<{}>>", self.typename)
			}
			FieldPlurality::Optional => format_args!("Option<{}>", self.typename),
			FieldPlurality::Plural if self.boxed => format_args!("Vec<Box<{}>>", self.typename),
			FieldPlurality::Plural => format_args!("Vec<{}>", self.typename),
			FieldPlurality::OptionalPlural if self.boxed => {
				format_args!("Option<Vec<Box<{}>>>", self.typename)
			}
			FieldPlurality::OptionalPlural => format_args!("Option<Vec<{}>>", self.typename),
		};

		// Output field
		write!(f, "#[doc = r#\"{}\"#]\n", self.docs.trim())?;
		if let Some(ref name) = self.rename {
			write!(f, "#[serde(rename = \"{name}\")]")?;
		}
		for attr in self.attrs.iter() {
			write!(f, "{attr}\n")?;
		}
		write!(f, "{}: {}", self.name, full_type)
	}
}

#[derive(Clone, Debug)]
pub struct Variant {
	pub name: Rc<str>,
	pub vtype: VariantType,
	pub docs: String,
	pub rename: Option<Rc<str>>,
	pub attrs: Vec<String>,
}
impl Display for Variant {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "#[doc = r#\"{}\"#]\n", self.docs.trim())?;
		for attr in self.attrs.iter() {
			write!(f, "{attr}\n")?;
		}

		// Output remainder based on variant.
		f.write_str(&self.name)?;
		match self.vtype {
			VariantType::Unit => f.write_str(",\n")?,
			VariantType::Newtype(ref field) => {
				let full_type = match field.plurality {
					FieldPlurality::None => format_args!("{}", field.typename),
					FieldPlurality::Optional => format_args!("Option<{}>", field.typename),
					FieldPlurality::Plural => format_args!("Vec<{}>", field.typename),
					FieldPlurality::OptionalPlural => {
						format_args!("Option<Vec<{}>>", field.typename)
					}
				};
				write!(f, "({}),\n", full_type)?;
			}
			VariantType::Tuple(ref fields) => {
				f.write_str("(\n")?;
				for field in fields.iter() {
					let full_type = match field.plurality {
						FieldPlurality::None => format_args!("{}", field.typename),
						FieldPlurality::Optional => format_args!("Option<{}>", field.typename),
						FieldPlurality::Plural => format_args!("Vec<{}>", field.typename),
						FieldPlurality::OptionalPlural => {
							format_args!("Option<Vec<{}>>", field.typename)
						}
					};
					write!(f, "\t{},\n", full_type)?;
				}
				f.write_str(")\n")?;
			}
			_ => {}
		};

		Ok(())
	}
}
#[derive(Clone, Debug)]
pub enum VariantType {
	Unit,
	// Only uses typename and plurality
	Newtype(Field),
	// Only uses typename and plurality.
	Tuple(Vec<Field>),
	// Only uses name, typename, plurality, docs, and attrs.
	Struct(Vec<Field>),
}
#[derive(Clone, Debug)]
pub enum TypeMembers {
	Alias(String),
	Fields(Vec<Field>),
	Variants(Vec<Variant>),
}
#[derive(Clone, Debug)]
pub struct RustType {
	pub name: Rc<str>,
	/// Whether this type should generate output in the crate. Usually true.
	pub output: bool,
	pub public: bool,
	/// Documentation without additional syntax.
	pub docs: String,
	pub rename: Option<Rc<str>>,
	/// Full attr strings without newline.
	pub attrs: Vec<String>,
	// Separate derive from attrs for easy modification.
	pub derive: Vec<String>,
	///
	pub any_features: BTreeSet<String>,
	pub extends: Option<Rc<str>>,
	pub members: TypeMembers,
	/// Complete impl block(s) with optional newline.
	pub impls: Vec<String>,
}
impl RustType {
	/// Returns `true` if this type is a struct of any kind.
	pub fn is_struct(&self) -> bool {
		match self.members {
			TypeMembers::Fields(_) => true,
			_ => false,
		}
	}

	/// Returns `true` if this type is a struct with a single unnamed field.
	pub fn is_newtype_struct(&self) -> bool {
		match self.members {
			TypeMembers::Fields(ref f) if f.len() == 1 && f[0].name.is_empty() => true,
			_ => false,
		}
	}

	/// If this type is a newtype struct, return the field.
	pub fn get_newtype_field(&self) -> Option<&Field> {
		match self.members {
			TypeMembers::Fields(ref f) if self.is_newtype_struct() => Some(&f[0]),
			_ => None,
		}
	}

	/// Returns `true` if this type is an enum of any kind.
	pub fn is_enum(&self) -> bool {
		match self.members {
			TypeMembers::Variants(_) => true,
			_ => false,
		}
	}
}
impl Display for RustType {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Don't do anything if ignoring output
		if !self.output {
			return Ok(());
		}

		let type_decl = match self.members {
			TypeMembers::Alias(_) => "type",
			TypeMembers::Fields(_) => "struct",
			TypeMembers::Variants(_) => "enum",
		};
		let pub_decl = match self.public {
			true => "pub ",
			false => "",
		};

		// Put docs first
		if self.docs.trim().len() > 0 {
			write!(f, "#[doc = r#\"{}\"#]\n", self.docs.trim())?;
		}

		// Derives
		if self.derive.len() > 0 {
			let derives = self.derive.join(", ");
			write!(f, "#[derive({derives})]\n")?;
		}

		// Attrs
		for attr in self.attrs.iter() {
			write!(f, "{attr}\n")?;
		}

		// Any features next
		let mut feat_block = None;
		if self.any_features.len() == 1 {
			let feat = self.any_features.iter().next().unwrap();
			feat_block = Some(format!("#[cfg(feature = \"{feat}\")]\n"));
		} else if self.any_features.len() > 1 {
			let mut feats = format!("#[cfg(any(\n");
			for feat in self.any_features.iter() {
				write!(feats, "\tfeature = \"{feat}\",\n")?;
			}
			write!(feats, "))]\n")?;
			feat_block = Some(feats);
		}

		// Type declaration
		if let Some(ref s) = feat_block {
			write!(f, "{s}")?;
		}
		write!(f, "{pub_decl}{type_decl} {}", self.name)?;

		// Fields and variants
		match self.members {
			TypeMembers::Alias(ref t) => write!(f, " = {t};\n"),
			TypeMembers::Fields(ref fields) => output_struct_fields(f, fields),
			TypeMembers::Variants(ref variants) => output_enum_variants(f, variants),
		}?;

		// Impls
		for imp in self.impls.iter() {
			if let Some(ref s) = feat_block {
				write!(f, "{s}")?;
			}
			write!(f, "{imp}\n")?;
		}

		Ok(())
	}
}

fn output_struct_fields(f: &mut fmt::Formatter, fields: &[Field]) -> fmt::Result {
	// If no fields, unit struct.
	if fields.is_empty() {
		return f.write_str(";\n");
	}

	// If type simple extends another type, output as newtype. Otherwise normal.
	if fields.len() == 1 && fields[0].name.is_empty() {
		let field = &fields[0];
		f.write_str("(\n")?;

		// Attributes (skip flatten since that's disallowed by serde).
		for attr in field
			.attrs
			.iter()
			.filter(|a| !a.trim_start().starts_with("#[serde(flatten)]"))
		{
			write!(f, "\t{attr}\n")?;
		}

		// Singular field
		// Prepare full type
		let full_type = match field.plurality {
			FieldPlurality::None if field.boxed => format_args!("Box<{}>", field.typename),
			FieldPlurality::None => format_args!("{}", field.typename),
			FieldPlurality::Optional if field.boxed => {
				format_args!("Option<Box<{}>>", field.typename)
			}
			FieldPlurality::Optional => format_args!("Option<{}>", field.typename),
			FieldPlurality::Plural if field.boxed => format_args!("Vec<Box<{}>>", field.typename),
			FieldPlurality::Plural => format_args!("Vec<{}>", field.typename),
			FieldPlurality::OptionalPlural if field.boxed => {
				format_args!("Option<Vec<Box<{}>>>", field.typename)
			}
			FieldPlurality::OptionalPlural => format_args!("Option<Vec<{}>>", field.typename),
		};
		let field_pub = match field.public {
			true => "pub",
			false => "",
		};

		write!(f, "\t{field_pub} {},\n", full_type)?;

		return f.write_str(");\n");
	} else {
		// Output struct content with named fields.
		f.write_str(" {\n")?;
		for field in fields {
			// Docs
			if field.docs.trim().len() > 0 {
				write!(f, "\t#[doc = r#\"{}\"#]\n", field.docs.trim())?;
			}

			// Rename
			if let Some(ref name) = field.rename {
				write!(f, "#[serde(rename = \"{name}\")]")?;
			}

			// Attributes
			for attr in field.attrs.iter() {
				write!(f, "\t{attr}\n")?;
			}

			// Field
			let full_type = match field.plurality {
				FieldPlurality::None if field.boxed => format_args!("Box<{}>", field.typename),
				FieldPlurality::None => format_args!("{}", field.typename),
				FieldPlurality::Optional if field.boxed => {
					format_args!("Option<Box<{}>>", field.typename)
				}
				FieldPlurality::Optional => format_args!("Option<{}>", field.typename),
				FieldPlurality::Plural if field.boxed => {
					format_args!("Vec<Box<{}>>", field.typename)
				}
				FieldPlurality::Plural => format_args!("Vec<{}>", field.typename),
				FieldPlurality::OptionalPlural if field.boxed => {
					format_args!("Option<Vec<Box<{}>>>", field.typename)
				}
				FieldPlurality::OptionalPlural => format_args!("Option<Vec<{}>>", field.typename),
			};
			let field_pub = match field.public {
				true => "pub",
				false => "",
			};

			write!(f, "\t{field_pub} {}: {},\n", field.name, full_type)?;
		}
	}

	f.write_str("}\n")
}

fn output_enum_variants(f: &mut fmt::Formatter, variants: &[Variant]) -> fmt::Result {
	f.write_str(" {\n")?;
	for variant in variants {
		// Docs
		if variant.docs.trim().len() > 0 {
			write!(f, "\t#[doc = r#\"{}\"#]\n", variant.docs.trim())?;
		}

		// Rename
		if let Some(ref name) = variant.rename {
			write!(f, "\t#[serde(rename = \"{name}\")]\n")?;
		}

		// Attributes
		for attr in variant.attrs.iter() {
			write!(f, "\t{attr}\n")?;
		}

		// Variant
		let variant_content = match variant.vtype {
			VariantType::Newtype(ref t) => match t.plurality {
				FieldPlurality::None if t.boxed => format_args!("(Box<{}>),", t.typename),
				FieldPlurality::None => format_args!("({}),", t.typename),
				FieldPlurality::Optional if t.boxed => {
					format_args!("(Option<Box<{}>>),", t.typename)
				}
				FieldPlurality::Optional => format_args!("(Option<{}>),", t.typename),
				FieldPlurality::Plural if t.boxed => {
					format_args!("(Vec<Box<{}>>),", t.typename)
				}
				FieldPlurality::Plural => format_args!("(Vec<{}>),", t.typename),
				FieldPlurality::OptionalPlural if t.boxed => {
					format_args!("(Option<Vec<Box<{}>>>),", t.typename)
				}
				FieldPlurality::OptionalPlural => {
					format_args!("(Option<Vec<{}>>),", t.typename)
				}
			},
			_ => format_args!(","),
		};
		write!(f, "\t{}{variant_content}\n", variant.name)?;
	}

	f.write_str("}\n")
}
