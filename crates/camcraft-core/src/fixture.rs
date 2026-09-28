//! A job the tests share.

/// A dome, roughed in Z levels leaving stock with a flat mill, then finished with
/// a ball in parallel passes; traverses 6 mm over the stock. Its model is called
/// `dome`, for the tests to supply.
pub(crate) const JOB: &str = r#"
version = 1
tolerance = 0.05

[tools.rough]
kind = "flat"
diameter = 6.0
number = 1

[tools.finish]
kind = "ball"
diameter = 4.0
number = 2

[[model]]
name = "dome"
path = "dome.stl"
units = "mm"

[[stock]]
name = "block"
kind = "around-models"
margin = [1.0, 1.0, 2.0]

[[setup]]
name = "top"
models = ["dome"]
stocks = ["block"]

[[setup.operation]]
name = "rough"
strategy = "pocket-clearing"
tool = "rough"
passes = { stepdown = 2.5, stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.5 } }
heights = { clearance = { ref = "stock-top", offset = 6.0 } }

[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "finish"
passes = { stepover = 0.8, feed = 2000.0 }
heights = { clearance = { ref = "stock-top", offset = 6.0 } }
"#;
