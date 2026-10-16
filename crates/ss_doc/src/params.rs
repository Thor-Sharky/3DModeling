//! Tool parameter values, stored with history entries so the last action can be adjusted.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Vec3([f32; 3]),
    Text(String),
    /// One of a fixed set of options, by key.
    Choice(String),
}

impl ParamValue {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            ParamValue::Float(v) => Some(*v as f32),
            ParamValue::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ParamValue::Int(v) => Some(*v),
            ParamValue::Float(v) => Some(v.round() as i64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            ParamValue::Text(s) | ParamValue::Choice(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<glam::Vec3> {
        match self {
            ParamValue::Vec3(v) => Some(glam::Vec3::from_array(*v)),
            _ => None,
        }
    }
}

pub type ParamValues = BTreeMap<String, ParamValue>;

/// "Which tool ran, with which settings" — lets the Adjust card re-run the last action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    pub tool: String,
    pub params: ParamValues,
}
