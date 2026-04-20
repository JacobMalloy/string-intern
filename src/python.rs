use pyo3::prelude::*;
use pyo3::types::PyString;

use crate::{Intern, InternC};

impl<'py> IntoPyObject<'py> for Intern {
    type Target = PyString;
    type Output = Bound<'py, PyString>;
    type Error = std::convert::Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(PyString::new(py, self.as_str()))
    }
}

impl<'py> IntoPyObject<'py> for &Intern {
    type Target = PyString;
    type Output = Bound<'py, PyString>;
    type Error = std::convert::Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(PyString::new(py, self.as_str()))
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for Intern {
    type Error = PyErr;

    fn extract(ob: pyo3::Borrowed<'a, 'py, PyAny>) -> Result<Self, Self::Error> {
        let py_str = ob.cast::<PyString>()?;
        let s = py_str.to_str()?;
        Ok(Intern::new(s))
    }
}

impl<'py> IntoPyObject<'py> for InternC {
    type Target = PyString;
    type Output = Bound<'py, PyString>;
    type Error = std::convert::Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(PyString::new(py, self.as_str()))
    }
}

impl<'py> IntoPyObject<'py> for &InternC {
    type Target = PyString;
    type Output = Bound<'py, PyString>;
    type Error = std::convert::Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(PyString::new(py, self.as_str()))
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for InternC {
    type Error = PyErr;

    fn extract(ob: pyo3::Borrowed<'a, 'py, PyAny>) -> Result<Self, Self::Error> {
        let py_str = ob.cast::<PyString>()?;
        let s = py_str.to_str()?;
        InternC::try_new(s).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }
}
