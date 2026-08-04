//! Functions to insert serde helper functions and such.
//!

use super::{Crate, Field, FieldPlurality};
use std::{
	fmt::Write,
	path::{Path, PathBuf},
};

/// If typename has a serde mod (for `serde(with = "")`), return the mod name.
pub fn has_custom_serde(typename: &str) -> Option<&str> {
	match typename {
		"chrono::NaiveTime" => Some("crate::serde_utils::naive_time"),
		"chrono::TimeDelta" => Some("crate::serde_utils::time_delta"),
		//"uuid::Uuid" => Some("uuid::serde::simple"),
		_ => None,
	}
}
/// If typename has a serde mod (for `serde(with = "")`), return the mod name.
pub fn add_custom_serde(field: &mut Field) -> bool {
	match field.typename.as_str() {
		"chrono::NaiveTime" => {
			let base_path = "crate::serde_utils::naive_time".to_string();
			let mod_path = match field.plurality {
				FieldPlurality::None => format_args!("{base_path}"),
				FieldPlurality::Optional => format_args!("{base_path}_opt"),
				FieldPlurality::Plural => format_args!("{base_path}_vec"),
				FieldPlurality::OptionalPlural => format_args!("{base_path}_opt_vec"),
			};
			field.attrs.push(format!("#[serde(with = \"{mod_path}\")]"));

			true
		}
		"chrono::TimeDelta" => {
			let base_path = "crate::serde_utils::time_delta";
			let mod_path = match field.plurality {
				FieldPlurality::None => format_args!("{base_path}"),
				FieldPlurality::Optional => format_args!("{base_path}_opt"),
				FieldPlurality::Plural => format_args!("{base_path}_vec"),
				FieldPlurality::OptionalPlural => format_args!("{base_path}_opt_vec"),
			};
			field.attrs.push(format!("#[serde(with = \"{mod_path}\")]"));

			true
		}
		/*"uuid::Uuid" => {
			let base_path = "crate::serde_utils::uuid_mod";
			let mod_path = match field.plurality {
				FieldPlurality::None => format_args!("{base_path}"),
				FieldPlurality::Optional => format_args!("{base_path}_opt"),
				FieldPlurality::Plural => format_args!("{base_path}_vec"),
				FieldPlurality::OptionalPlural => format_args!("{base_path}_opt_vec"),
			};
			field.attrs.push(format!("#[serde(with = \"{mod_path}\")]"));

			true
		}*/
		_ => false,
	}
}

pub fn add_utils(cr8: &mut Crate) {
	// Add module declaration to root.
	let lib = cr8.contents.get_mut(Path::new("crate")).unwrap();
	lib.modules
		.insert(format!("#[macro_use]\npub mod serde_utils;"));

	// Create module file
	let m = cr8
		.contents
		.entry(PathBuf::from("crate/serde_utils"))
		.or_default();
	_ = write!(m.docs, "Module with serde helpers and utilities.");

	// Paste macro for macros.
	m.imports.insert("use paste::paste;".to_string());

	cr8.deps.push("iso8601 = \"0.6.3\"".to_owned());

	// Insert everything
	m.fns.push(SERDE_MOD_EXTRAS_MACRO.to_owned());
	m.fns.push(NAIVETIME_MODS.to_owned());
	m.fns.push(TIMEDELTA_MOD.to_owned());
	m.fns.push(ENUM_SERDE_MACRO.to_owned());
}

const SERDE_MOD_EXTRAS_MACRO: &str = r#"
/// Creates serde modules for [`Option<T>`] and [`Vec<T>`], where `T` is given
/// by `$target`, and forwards the serde work to the given module `$base_mod`.
///
/// The resulting modules are named after the base module but with "_opt" and
/// "_vec" suffixes for [`Option<T>`] and [`Vec<T>`] wrapped types,
/// respectively.
macro_rules! serde_mod_extras {
	($base_mod:path, $target:ty) => {
		// Option-wrapped type module.
		paste! {
		pub mod [< $base_mod _opt >] {
			use super::$base_mod;
			use serde::{de::{Error, Visitor}, Deserializer, Serialize, Serializer};
			use std::fmt;

			type SerdeType = Option<$target>;

			pub(super) struct Wrapper<'a>(pub &'a $target);
			impl<'a> Serialize for Wrapper<'a> {
				fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
					$base_mod::serialize(self.0, ser)
				}
			}

			pub fn serialize<S: Serializer>(t: &SerdeType, ser: S) -> Result<S::Ok, S::Error> {
				match t.as_ref() {
					Some(v) => {
						ser.serialize_some(&Wrapper(v))
					},
					None => ser.serialize_none(),
				}
			}
			pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<SerdeType, D::Error> {
				de.deserialize_option(TypeVisitor)
			}

			struct TypeVisitor;
			impl<'de> Visitor<'de> for TypeVisitor {
				type Value = SerdeType;

				fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
					f.write_str(concat!("optional ", stringify!($target)))
				}

				fn visit_none<E: Error>(self) -> Result<SerdeType, E> {
					Ok(None)
				}
				fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<SerdeType, D::Error> {
					$base_mod::deserialize(deserializer).map(|v| Some(v))
				}
			}
		}

		// Vec-wrapped type module.
		pub mod [< $base_mod _vec >] {
			// Reuse `Wrapper` type from opt mod.
			use super::$base_mod;
			use super::[< $base_mod _opt >] ::Wrapper;
			use serde::{
				de::{DeserializeSeed, SeqAccess, Visitor},
				ser::SerializeSeq,
				Deserializer,
				Serializer
			};
			use std::fmt;

			type SerdeType = Vec<$target>;

			pub fn serialize<S: Serializer>(t: &SerdeType, ser: S) -> Result<S::Ok, S::Error> {
				let mut seq = ser.serialize_seq(Some(t.len()))?;
				for v in t.iter() {
					seq.serialize_element(&Wrapper(v))?;
				}
				seq.end()
			}
			pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<SerdeType, D::Error> {
				de.deserialize_seq(TypeVisitor)
			}

			struct TypeSeed;
			impl<'de> DeserializeSeed<'de> for TypeSeed {
				type Value = $target;

				fn deserialize<D: Deserializer<'de>>(self, de: D) -> Result<Self::Value, D::Error> {
					$base_mod::deserialize(de)
				}
			}

			struct TypeVisitor;
			impl<'de> Visitor<'de> for TypeVisitor {
				type Value = SerdeType;

				fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
					f.write_str(concat!("sequence of ", stringify!($target)))
				}

				fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<SerdeType, A::Error> {
					let mut values = match seq.size_hint() {
						Some(s) => Vec::with_capacity(s),
						None => Vec::new(),
					};

					while let Some(v) = seq.next_element_seed(TypeSeed)? {
						values.push(v);
					}

					Ok(values)
				}
			}
		}
		}
	};
}"#;

const NAIVETIME_MODS: &str = r#"
/// (De)Serializer for [chrono::NaiveTime] that add/strips the 'Z' suffix from
/// the timestamp.
pub mod naive_time {
	use chrono::NaiveTime;
	use serde::{de::Error, Deserialize, Deserializer, Serializer};

	pub fn get_raw_value(t: &NaiveTime) -> String {
		let mut st = t.to_string();
		st.push('Z');
		st
	}
	pub fn serialize<S: Serializer>(t: &NaiveTime, ser: S) -> Result<S::Ok, S::Error> {
		ser.serialize_str(get_raw_value(t).as_str())
	}
	pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<NaiveTime, D::Error> {
		let st = String::deserialize(de)?;
		if let Some(s) = st.strip_suffix('Z') {
			match s.parse() {
				Err(e) => Err(D::Error::custom(e)),
				Ok(v) => Ok(v),
			}
		} else {
			Err(D::Error::custom("Expected 'Z' tz suffix"))
		}
	}
}

// For the Option- and Vec- wrapped variants.
serde_mod_extras!(naive_time, chrono::NaiveTime);
"#;

const TIMEDELTA_MOD: &str = r#"
/// (De)Serializer for [`TimeDelta`](chrono::TimeDelta) that uses
/// [`Duration`](iso8601::Duration) to parse, then converts to/from
/// [`TimeDelta`](chrono::TimeDelta).
pub mod time_delta {
	use std::str::FromStr;
	use chrono::TimeDelta;
	use iso8601::Duration;
	use serde::{de::Error, Deserialize, Deserializer, Serializer};

	pub fn get_raw_value(t: &TimeDelta) -> String {
		t.to_string()
	}
	pub fn serialize<S: Serializer>(t: &TimeDelta, ser: S) -> Result<S::Ok, S::Error> {
		ser.serialize_str(get_raw_value(t).as_str())
	}
	pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<TimeDelta, D::Error> {
		let time_string = String::deserialize(de)?;

		// From [Gregorian calendar](https://en.wikipedia.org/wiki/Gregorian_calendar#Accuracy).
		// Lower error than `365.2425` per year.
		const SECS_PER_DAY: i64 = 3600 * 24;
		const SECS_PER_HOUR: i64 = 3600;
		const SECS_PER_MIN: i64 = 60;
		const AVG_DAYS_PER_YEAR: f64 = 365.24237;
		const AVG_DAYS_PER_MONTH: f64 = AVG_DAYS_PER_YEAR / 12.0;
		const AVG_SECS_PER_YEAR: i64 = (AVG_DAYS_PER_YEAR * SECS_PER_DAY as f64) as _;
		const AVG_SECS_PER_MONTH: i64 = (AVG_DAYS_PER_MONTH * SECS_PER_DAY as f64) as _;

		// Largest `s` such that `s * 1000 + 2^32 < i64::MAX`.
		const MAX_SECS_FOR_MILLIS: i64 = 9223372032559808;

		match Duration::from_str(&time_string) {
			Ok(d) => {
				Ok(match d {
					Duration::YMDHMS { year, month, day, hour, minute, second, millisecond } => {
						let total_secs = year as i64 * AVG_SECS_PER_YEAR
							+ month as i64 * AVG_SECS_PER_MONTH
							+ day as i64 * SECS_PER_DAY
							+ hour as i64 * SECS_PER_HOUR
							+ minute as i64 * SECS_PER_MIN
							+ second as i64;

						// If there's room for
						if total_secs <= MAX_SECS_FOR_MILLIS {
							let total_msec = total_secs * 1000 + millisecond as i64;
							TimeDelta::milliseconds(total_msec)
						} else {
							TimeDelta::seconds(total_secs)
						}
					}
					Duration::Weeks(w) => TimeDelta::weeks(w as _),
				})
			}
			Err(e) => Err(D::Error::custom(e))
		}
	}
}

// For the Option- and Vec- wrapped variants.
serde_mod_extras!(time_delta, chrono::TimeDelta);
"#;

const UUID_MOD: &str = r#"
/// Alies for the following Uuid wrapper mods.
pub(crate) use uuid::serde::simple as uuid_mod;

// For the Option- and Vec- wrapped variants.
serde_mod_extras!(uuid_mod, uuid::Uuid);
"#;

const ENUM_SERDE_MACRO: &str = r#"
/// Macro for ease of (de)serialize of choicetype enums, specifically as a
/// work-around when using `quick-xml`.
// `$en_name` is the enum identifier, which is what gets passed to serde.
// `$vnt_name` is the actual variant name in the enum.
// `$vnt_ser_name` is the serialized name of the variant (like serde rename).
#[macro_export]
macro_rules! struct_like_serde {
	{
		$en_name:ident
		$(
			$(#[$vnt_type:ty => $serde_with_mod:path])?
			$vnt_name:ident -> $vnt_ser_name:literal $(,)?
		)+
	} => {
		// Impl Serialize
		impl serde::Serialize for $en_name {
			fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
				use serde::ser::SerializeMap;
				use paste::paste;

				let mut map = ser.serialize_map(Some(1))?;
				match self {
					$(paste![Self :: $vnt_name (v)] => {
						$(
						use serde::ser::{Serializer, Serialize};
						struct Wrapper<'a>(&'a $vnt_type);
						impl<'a> Serialize for Wrapper<'a> {
							fn serialize<S: Serializer>(&self, se: S) -> Result<S::Ok, S::Error> {
								paste![$serde_with_mod :: serialize ( self.0 , se )]
							}
						}
						let v = &Wrapper(v);
						)?

						map.serialize_entry($vnt_ser_name, v)
					}),+
				}?;

				map.end()
			}
		}

		impl<'de> serde::Deserialize<'de> for $en_name {
			fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
				use core::fmt;
				use serde::de::{Error, MapAccess, Visitor};
				use paste::paste;

				const FIELDS: &[&str] = &[
					$( $vnt_ser_name, )+
				];

				struct EnumVisitor;
				impl<'v> Visitor<'v> for EnumVisitor {
					type Value = $en_name;

					fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
						f.write_str("1 variant")
					}

					fn visit_map<A: MapAccess<'v>>(self, mut map: A) -> Result<Self::Value, A::Error> {
						let en = match map.next_key()? {
							$(
							Some($vnt_ser_name) => {
								use core::marker::PhantomData;
								let d = PhantomData;
								$(
								use serde::de::{Deserializer, DeserializeSeed};

								struct Seed;
								impl<'de> DeserializeSeed<'de> for Seed {
									type Value = $vnt_type;

									fn deserialize<D: Deserializer<'de>>(self, de: D) -> Result<Self::Value, D::Error> {
										paste![$serde_with_mod :: deserialize ( de )]
									}
								}
								// `_p` helps compiler infer type of the unused `d` from above.
								let _p: PhantomData<$vnt_type> = d;
								let d = Seed;
								)?

								paste! [$en_name :: $vnt_name (map.next_value_seed(d)?)]
							}
							)+
							Some(v) => return Err(A::Error::unknown_variant(v, FIELDS)),
							_ => return Err(A::Error::missing_field("Any valid variant"))
						};

						// Empty the `map`.
						let mut total = 1;
						while let Some(_) = map.next_entry::<(),()>()? {
							total += 1;
						}

						if total != 1 {
							Err(A::Error::invalid_length(total, &"enum with 1 variant."))
						} else {
							Ok(en)
						}
					}
				}

				de.deserialize_map(EnumVisitor)
			}
		}
	};
}"#;
