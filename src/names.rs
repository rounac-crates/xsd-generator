//! Utilities for handling type names and case conversions.
//!

use xsd_generator::{ContentType, Element, ExtRest};

static RUST_KEYWORDS: &[&str] = &[
	"_", "abstract", "as", "async", "await", "become", "box", "break", "const", "continue",
	"crate", "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if",
	"impl", "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub",
	"ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
	"typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Convert a string to a proper Rust struct/enum/trait name.
pub fn to_pascal_case(name: &str) -> Option<String> {
	let mut new_string = String::with_capacity(name.len());
	let mut in_word = true;
	for c in name.chars() {
		// Punctuation (mainly '_' and '-') delimits words.
		if c.is_ascii_punctuation() {
			in_word = true;
			continue;
		}

		// Ignore non-alphabetical and only permit latin digits.
		if !(c.is_alphabetic() || c.is_ascii_digit()) {
			continue;
		}

		// Uppercase starts new words, unless preceeded by an uppercase.
		if c.is_uppercase() && in_word {
			new_string.extend(c.to_uppercase());
			in_word = false;
		} else if c.is_lowercase() {
			new_string.push(c);
			in_word = true;
		} else {
			new_string.extend(c.to_lowercase());
		}
	}

	// Compare old to new to indicate whether change was made.
	if *name == new_string {
		None
	} else {
		Some(new_string)
	}
}

/// Convert a string to snake case for fields.
pub fn to_snake_case(name: &str) -> Option<String> {
	let mut new_string = String::with_capacity(name.len());
	let mut in_word = true;
	for c in name.chars() {
		// Punctuation (mainly '_' and '-') delimits words.
		if c.is_ascii_punctuation() {
			in_word = true;
			continue;
		}

		// Ignore non-alphabetical and only permit latin digits.
		if !(c.is_alphabetic() || c.is_ascii_digit()) {
			continue;
		}

		// Uppercase starts new words, unless preceeded by an uppercase.
		if c.is_uppercase() && in_word {
			if !new_string.is_empty() {
				new_string.push('_');
			}
			in_word = false;
		} else if c.is_lowercase() {
			in_word = true;
		}

		// Always push lowercase to string.
		new_string.extend(c.to_lowercase());
	}

	// Compare old to new to indicate whether change was made.
	if *name == new_string {
		None
	} else {
		Some(new_string)
	}
}

/// Convert a string to snake case for fields.
pub fn to_uppercase_underscore(name: &str) -> Option<String> {
	let mut new_string = String::with_capacity(name.len());
	let mut in_word = true;
	for c in name.chars() {
		// Any punctuation will count as delimeter, so underscore.
		// Otherwise ignore any non-alphabetic or digit chars.
		if c.is_ascii_punctuation() {
			new_string.push('_');
			continue;
		} else if !(c.is_alphabetic() || c.is_ascii_digit()) {
			continue;
		}

		// Uppercase starts new words, unless preceeded by an uppercase.
		if c.is_uppercase() && in_word {
			if !new_string.is_empty() {
				new_string.push('_');
			}
			in_word = false;
		} else if c.is_lowercase() {
			in_word = true;
		}

		// Always push lowercase to string.
		new_string.extend(c.to_uppercase());
	}

	// Compare old to new to indicate whether change was made.
	if *name == new_string {
		None
	} else {
		Some(new_string)
	}
}

/// If `name` is an illegal identifier, modify to make it legal.
pub fn fix_illegal_name(name: &mut String) -> bool {
	let mut res = false;

	// If name starts with number, prepend underscore ('_').
	if let Some(c) = name.chars().next() {
		if c.is_ascii_digit() {
			res = true;
			name.insert(0, '_');
		}
	}

	// If name is keyword, append underscore.
	if let Ok(_) = RUST_KEYWORDS.binary_search(&name.as_str()) {
		res = true;
		name.push('_');
	}

	res
}

/// Convert all type names and field names to correct case.
pub fn rustify_names(elems: &mut [Element]) {
	for elem in elems {
		// All type names are pascal.
		to_pascal_case(&mut elem.name);
		fix_illegal_name(&mut elem.name);

		if let Some(tn) = elem.type_name.as_mut() {
			to_pascal_case(tn);
			fix_illegal_name(tn);
		}

		// Choice types become enums, so pascal case. Otherwise snake.
		match elem.type_ {
			Some(ContentType::Choice) => {
				for f in elem.values.iter_mut() {
					to_pascal_case(&mut f.name);
					fix_illegal_name(&mut f.name);
					to_pascal_case(f.type_name.as_mut().unwrap());
					fix_illegal_name(f.type_name.as_mut().unwrap());
				}
			}
			_ => {
				for f in elem.values.iter_mut() {
					to_snake_case(&mut f.name);
					fix_illegal_name(&mut f.name);
					to_pascal_case(&mut f.type_name.as_mut().unwrap());
					fix_illegal_name(&mut f.type_name.as_mut().unwrap());
				}
			}
		};

		match elem.ext_rest {
			Some(ExtRest::Extend(ref mut s)) => _ = to_pascal_case(s),
			Some(ExtRest::Restrict(ref mut r)) => {
				// Base type proper name
				to_pascal_case(&mut r.base);
				fix_illegal_name(&mut r.base);

				// All enum variant names are pascal.
				for v in r.enumeration.iter_mut() {
					to_pascal_case(&mut v.value);
					fix_illegal_name(&mut v.value);
				}
			}
			_ => {}
		};
	}
}

/// Convert schema types into actual types.
pub fn strip_xsd_ns(elems: &mut [Element]) {
	for elem in elems {
		// This should only affect top-level elements
		if let Some(ty) = elem.type_name.as_mut() {
			if let Some(colon) = ty.rfind(':') {
				ty.replace_range(..=colon, "");
			}
		}

		for f in elem.values.iter_mut() {
			if let Some(ref mut tname) = f.type_name {
				// First, strip everything up to the last colon (XML namespace).
				if let Some(colon) = tname.rfind(':') {
					tname.replace_range(..=colon, "");
				}
			}
		}

		match elem.ext_rest {
			Some(ExtRest::Extend(ref mut p)) => {
				if let Some(colon) = p.rfind(':') {
					p.replace_range(..=colon, "");
				}
			}
			Some(ExtRest::Restrict(ref mut r)) => {
				if let Some(colon) = r.base.rfind(':') {
					r.base.replace_range(..=colon, "");
				}
			}
			_ => {}
		}
	}
}
