use crate::{Result, TuningError};
use std::collections::BTreeMap;
use windows::{
    Win32::{
        Foundation::VARIANT_TRUE,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
            },
            Ole::{
                SafeArrayCreateVector, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
                SafeArrayPutElement,
            },
            Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE},
            Variant::{
                VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_ARRAY, VT_BOOL, VT_BSTR, VT_I1,
                VT_I4, VT_UI2, VT_UI4, VariantClear,
            },
            Wmi::{
                IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices,
                WBEM_FLAG_CREATE_ONLY, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
                WBEM_FLAG_UPDATE_ONLY, WBEM_INFINITE, WbemLocator,
            },
        },
    },
    core::{BSTR, PCWSTR},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    String(String),
    Strings(Vec<String>),
    Number(i64),
    Bool(bool),
    Empty,
}

pub(super) struct Row {
    object: IWbemClassObject,
    pub fields: BTreeMap<String, Value>,
}

impl Row {
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.fields.get(name) {
            Some(Value::String(value)) => Some(value),
            _ => None,
        }
    }

    pub fn strings(&self, name: &str) -> Option<&[String]> {
        match self.fields.get(name) {
            Some(Value::Strings(value)) => Some(value),
            _ => None,
        }
    }

    pub fn number(&self, name: &str) -> Option<i64> {
        match self.fields.get(name) {
            Some(Value::Number(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn boolean(&self, name: &str) -> Option<bool> {
        match self.fields.get(name) {
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        }
    }
}

pub(super) struct Session {
    services: IWbemServices,
    uninitialize: bool,
}

impl Session {
    pub fn connect() -> Result<Self> {
        unsafe {
            let uninitialize = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| native("create WMI locator", error))?;
            let services = locator
                .ConnectServer(
                    &BSTR::from("ROOT\\StandardCimv2"),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .map_err(|error| native("connect StandardCimv2", error))?;
            CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                None,
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )
            .map_err(|error| native("secure WMI proxy", error))?;
            Ok(Self {
                services,
                uninitialize,
            })
        }
    }

    pub fn query(&self, class: &str, fields: &[&str], condition: Option<&str>) -> Result<Vec<Row>> {
        let suffix = condition
            .map(|value| format!(" WHERE {value}"))
            .unwrap_or_default();
        let query = format!("SELECT {} FROM {class}{suffix}", fields.join(","));
        let enumerator = unsafe {
            self.services.ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
        }
        .map_err(|error| native("query native CIM provider", error))?;
        collect(enumerator, fields)
    }

    pub fn create(&self, class: &str, values: &[(&str, Value)]) -> Result<()> {
        let instance = unsafe { self.get_object(class)?.SpawnInstance(0) }
            .map_err(|error| native("create CIM instance", error))?;
        put_values(&instance, values)?;
        unsafe {
            self.services.PutInstance(
                &instance,
                windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_CREATE_ONLY.0),
                None,
                None,
            )
        }
        .map_err(|error| native("create provider instance", error))
    }

    pub fn update(&self, row: &Row, values: &[(&str, Value)]) -> Result<()> {
        put_values(&row.object, values)?;
        unsafe {
            self.services.PutInstance(
                &row.object,
                windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_UPDATE_ONLY.0),
                None,
                None,
            )
        }
        .map_err(|error| native("update provider instance", error))
    }

    pub fn delete(&self, row: &Row) -> Result<()> {
        let path = required_path(row, "delete provider instance")?;
        unsafe {
            self.services
                .DeleteInstance(&BSTR::from(path), Default::default(), None, None)
        }
        .map_err(|error| native("delete provider instance", error))
    }

    pub fn invoke(
        &self,
        row: &Row,
        class: &str,
        method: &str,
        values: &[(&str, Value)],
    ) -> Result<()> {
        let path = required_path(row, "invoke provider method")?;
        let class_object = self.get_object(class)?;
        let mut signature = None;
        unsafe {
            class_object.GetMethod(
                PCWSTR::from_raw(wide(method).as_ptr()),
                0,
                &mut signature,
                std::ptr::null_mut(),
            )
        }
        .map_err(|error| native("read provider method signature", error))?;
        let signature = signature.ok_or_else(|| {
            provider_error(
                "read provider method signature",
                "provider returned no input signature",
            )
        })?;
        let input = unsafe { signature.SpawnInstance(0) }
            .map_err(|error| native("create provider method input", error))?;
        put_values(&input, values)?;
        let mut output = None;
        unsafe {
            self.services.ExecMethod(
                &BSTR::from(path),
                &BSTR::from(method),
                Default::default(),
                None,
                &input,
                Some(&mut output),
                None,
            )
        }
        .map_err(|error| native("invoke provider method", error))?;
        let output = output.ok_or_else(|| {
            provider_error(
                "invoke provider method",
                "provider returned no method output object",
            )
        })?;
        let code = match read_value(&output, "ReturnValue")? {
            Some(Value::Number(code)) => code,
            Some(_) => {
                return Err(provider_error(
                    "invoke provider method",
                    "provider returned a non-numeric ReturnValue",
                ));
            }
            None => {
                return Err(provider_error(
                    "invoke provider method",
                    "provider omitted required ReturnValue",
                ));
            }
        };
        if code != 0 {
            return Err(TuningError::Native {
                operation: "invoke provider method",
                code,
                message: format!("{class}.{method} returned failure"),
            });
        }
        Ok(())
    }

    fn get_object(&self, path: &str) -> Result<IWbemClassObject> {
        let mut object = None;
        unsafe {
            self.services.GetObject(
                &BSTR::from(path),
                Default::default(),
                None,
                Some(&mut object),
                None,
            )
        }
        .map_err(|error| native("read provider class", error))?;
        object.ok_or_else(|| provider_error("read provider class", "provider returned no class"))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() }
        }
    }
}

fn required_path<'a>(row: &'a Row, operation: &'static str) -> Result<&'a str> {
    row.string("__PATH")
        .ok_or_else(|| provider_error(operation, "provider did not return __PATH"))
}

fn collect(enumerator: IEnumWbemClassObject, fields: &[&str]) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    loop {
        let mut objects = [None];
        let mut returned = 0;
        let status = unsafe { enumerator.Next(WBEM_INFINITE, &mut objects, &mut returned) };
        if status.is_err() {
            return Err(native("enumerate native CIM provider", status.into()));
        }
        if returned == 0 {
            break;
        }
        let object = objects[0].take().ok_or_else(|| {
            provider_error(
                "enumerate native CIM provider",
                "provider returned an empty row",
            )
        })?;
        let mut values = BTreeMap::new();
        for field in fields {
            if let Some(value) = read_value(&object, field)? {
                values.insert((*field).into(), value);
            }
        }
        rows.push(Row {
            object,
            fields: values,
        });
        if rows.len() > 4096 {
            return Err(provider_error(
                "enumerate native CIM provider",
                "provider row limit exceeded",
            ));
        }
    }
    Ok(rows)
}

fn read_value(object: &IWbemClassObject, name: &str) -> Result<Option<Value>> {
    let mut value = VARIANT::default();
    unsafe {
        object.Get(
            PCWSTR::from_raw(wide(name).as_ptr()),
            0,
            &mut value,
            None,
            None,
        )
    }
    .map_err(|error| native("read provider property", error))?;
    let result = unsafe {
        let inner = &value.Anonymous.Anonymous;
        match inner.vt {
            VT_BSTR => Value::String((*inner.Anonymous.bstrVal).to_string()),
            VT_BOOL => Value::Bool(inner.Anonymous.boolVal == VARIANT_TRUE),
            VT_I1 => Value::Number(i64::from(inner.Anonymous.cVal)),
            VT_I4 => Value::Number(i64::from(inner.Anonymous.lVal)),
            VT_UI2 => Value::Number(i64::from(inner.Anonymous.uiVal)),
            VT_UI4 => Value::Number(i64::from(inner.Anonymous.ulVal)),
            kind if kind == (VT_ARRAY | VT_BSTR) => {
                Value::Strings(read_string_array(inner.Anonymous.parray)?)
            }
            kind if kind.0 == 0 || kind.0 == 1 => Value::Empty,
            kind => {
                return Err(TuningError::Native {
                    operation: "read provider property",
                    code: i64::from(kind.0),
                    message: format!("unsupported VARIANT type for {name}"),
                });
            }
        }
    };
    unsafe { VariantClear(&mut value) }
        .map_err(|error| native("clear provider property", error))?;
    Ok((result != Value::Empty).then_some(result))
}

unsafe fn read_string_array(
    array: *mut windows::Win32::System::Com::SAFEARRAY,
) -> Result<Vec<String>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let lower = unsafe { SafeArrayGetLBound(array, 1) }
        .map_err(|error| native("read provider array", error))?;
    let upper = unsafe { SafeArrayGetUBound(array, 1) }
        .map_err(|error| native("read provider array", error))?;
    let mut values = Vec::new();
    for index in lower..=upper {
        let mut value = BSTR::new();
        unsafe { SafeArrayGetElement(array, &index, &mut value as *mut _ as _) }
            .map_err(|error| native("read provider array item", error))?;
        values.push(value.to_string());
    }
    Ok(values)
}

fn put_values(object: &IWbemClassObject, values: &[(&str, Value)]) -> Result<()> {
    for (name, value) in values {
        let mut variant = make_variant(value)?;
        unsafe { object.Put(PCWSTR::from_raw(wide(name).as_ptr()), 0, &variant, 0) }
            .map_err(|error| native("set provider property", error))?;
        unsafe { VariantClear(&mut variant) }
            .map_err(|error| native("clear provider input", error))?;
    }
    Ok(())
}

fn make_variant(value: &Value) -> Result<VARIANT> {
    let (kind, data) = match value {
        Value::String(value) => (
            VT_BSTR,
            VARIANT_0_0_0 {
                bstrVal: std::mem::ManuallyDrop::new(BSTR::from(value)),
            },
        ),
        Value::Number(value) => (
            VT_I4,
            VARIANT_0_0_0 {
                lVal: i32::try_from(*value)
                    .map_err(|_| TuningError::Validation("provider number out of range".into()))?,
            },
        ),
        Value::Bool(value) => (
            VT_BOOL,
            VARIANT_0_0_0 {
                boolVal: if *value {
                    VARIANT_TRUE
                } else {
                    Default::default()
                },
            },
        ),
        Value::Strings(values) => {
            let array = unsafe {
                SafeArrayCreateVector(
                    VT_BSTR,
                    0,
                    u32::try_from(values.len())
                        .map_err(|_| TuningError::Validation("provider array too large".into()))?,
                )
            };
            if array.is_null() {
                return Err(provider_error("create provider array", "allocation failed"));
            }
            for (index, value) in values.iter().enumerate() {
                let bstr = BSTR::from(value);
                unsafe { SafeArrayPutElement(array, &(index as i32), bstr.as_ptr() as _) }
                    .map_err(|error| native("write provider array item", error))?;
            }
            (VT_ARRAY | VT_BSTR, VARIANT_0_0_0 { parray: array })
        }
        Value::Empty => {
            return Err(TuningError::Validation(
                "empty provider value cannot be written".into(),
            ));
        }
    };
    Ok(VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: kind,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: data,
            }),
        },
    })
}

fn native(operation: &'static str, error: windows::core::Error) -> TuningError {
    TuningError::Native {
        operation,
        code: i64::from(error.code().0),
        message: error.message(),
    }
}

fn provider_error(operation: &'static str, message: &str) -> TuningError {
    TuningError::Native {
        operation,
        code: -1,
        message: message.into(),
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
