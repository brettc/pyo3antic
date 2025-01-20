use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ImplItem, ItemImpl};

#[proc_macro_attribute]
pub fn pydantic_schema(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut input_impl = parse_macro_input!(input as ItemImpl);
    let ty = &input_impl.self_ty;

    // Create the pydantic schema method
    let schema_method: ImplItem = syn::parse_quote! {
        #[classmethod]
        fn __get_pydantic_core_schema__(
            cls: &Bound<'_, PyType>,
            _source_type: &Bound<'_, PyAny>,
            _handler: &Bound<'_, PyAny>,
        ) -> PyResult<PyObject> {
            Python::with_gil(|py| {
                let core_schema = py.import("pydantic_core.core_schema")?.unbind();
                let schema = core_schema.call_method1(
                    py,
                    "union_schema",
                    (PyList::new(
                        py,
                        &[
                            core_schema.call_method1(py, "is_instance_schema", (cls,))?,
                            core_schema.call_method(
                                py,
                                "dict_schema",
                                (),
                                Some(
                                    &[
                                        ("keys_schema", core_schema.call_method0(py, "str_schema")?),
                                        ("values_schema", core_schema.call_method0(py, "any_schema")?),
                                    ]
                                    .into_py_dict(py)?,
                                ),
                            )?,
                        ],
                    )?,),
                )?;

                let validate_fun = |args: &Bound<'_, PyTuple>,
                                  _kwargs: Option<&Bound<'_, PyDict>>|
                 -> PyResult<Py<#ty>> {
                    let value = args.get_item(0)?;
                    Python::with_gil(|py| {
                        if value.is_instance_of::<#ty>() {
                            Ok(Py::new(py, value.extract::<#ty>()?)?)
                        } else {
                            match depythonize::<#ty>(&value) {
                                Ok(instance) => Ok(Py::new(py, instance)?),
                                Err(e) => Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                                    format!("Failed to deserialize: {}", e),
                                )),
                            }
                        }
                    })
                };
                let validate = PyCFunction::new_closure(py, None, None, validate_fun).unwrap();

                let serialize_fun = |args: &Bound<'_, PyTuple>,
                                   _kwargs: Option<&Bound<'_, PyDict>>|
                 -> PyResult<Py<PyAny>> {
                    let value = args.get_item(0)?;
                    Python::with_gil(|py| {
                        match value.extract::<#ty>() {
                            Ok(instance) => match pythonize(py, &instance) {
                                Ok(py_dict) => Ok(py_dict.unbind()),
                                Err(e) => Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                                    format!("Failed to serialize: {}", e),
                                )),
                            },
                            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                                format!("Failed to extract: {}", e),
                            )),
                        }
                    })
                };
                let serialize = PyCFunction::new_closure(py, None, None, serialize_fun).unwrap();

                let final_schema = core_schema.call_method(
                    py,
                    "no_info_after_validator_function",
                    (validate, schema),
                    Some(
                        &[(
                            "serialization",
                            core_schema.call_method1(
                                py,
                                "plain_serializer_function_ser_schema",
                                (serialize,),
                            )?,
                        )]
                        .into_py_dict(py)?,
                    ),
                )?;
                Ok(final_schema)
            })
        }
    };

    // Add the schema method to the existing implementation items
    input_impl.items.push(schema_method);

    // Generate the final output with necessary imports
    TokenStream::from(quote! {
        use pyo3::prelude::*;
        use pyo3::types::*;
        use pythonize::{pythonize, depythonize};
        use pyo3::types::{IntoPyDict, PyType, PyDict, PyList, PyTuple};
        use pyo3::exceptions::PyValueError;

        #input_impl
    })
}
