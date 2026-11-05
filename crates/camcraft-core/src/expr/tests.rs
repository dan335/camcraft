use super::*;

/// The values a test's expressions read: a quarter-inch ball, and a
/// tolerance.
fn values(name: &str) -> Option<Value> {
    Some(match name {
        "tool_diameter" => Value::Number(6.35),
        "tool_fluteLength" => Value::Number(19.05),
        "tool_cornerRadius" => Value::Number(3.175),
        "tolerance" => Value::Number(0.01),
        "tool_type" => Value::Text("ball end mill".into()),
        _ => return None,
    })
}

fn eval(text: &str) -> Result<f64, ExprError> {
    evaluate(text, &values)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// Fusion's own defaults read as Fusion has them.
#[test]
fn fusions_expressions_are_worked_out() {
    assert!(near(eval("tool_diameter * 0.4").unwrap(), 2.54));
    assert!(near(
        eval("Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)").unwrap(),
        14.2875
    ));
    assert!(near(eval("Math.min(tool_diameter; 12mm)").unwrap(), 6.35));
    assert!(near(eval("tolerance <= 0.02 ? 1mm : 3mm").unwrap(), 1.0));
    assert!(near(
        eval("tool_type == 'face mill' ? tool_diameter * 0.95 : tool_diameter * 0.4").unwrap(),
        2.54
    ));
    assert!(near(eval("(tool_diameter/2)^2").unwrap(), 3.175 * 3.175));
}

/// Numbers take a length unit, worked out in millimetres; and the usual
/// order holds: powers, then products, then sums, then comparisons.
#[test]
fn units_and_precedence() {
    assert!(near(eval("1in").unwrap(), 25.4));
    assert!(near(eval("0.5 in + 2mm").unwrap(), 14.7));
    assert!(near(eval("1cm").unwrap(), 10.0));
    assert!(near(eval("2 + 3 * 4").unwrap(), 14.0));
    assert!(near(eval("-2^2").unwrap(), -4.0));
    assert!(near(eval("2^3^2").unwrap(), 512.0), "right to left");
    assert!(near(eval("10 - 4 - 3").unwrap(), 3.0), "left to right");
    assert!(near(eval("1e-3 * 1000").unwrap(), 1.0));
    assert!(near(eval("Math.roundto(1.23456; 0.01)").unwrap(), 1.23));
    assert!(near(eval("Math.max(1, 5, 3)").unwrap(), 5.0), "commas too");
    assert!(near(eval("!(1 > 2) && 3 >= 3 ? 7 : 8").unwrap(), 7.0));
}

/// What cannot be worked out says why, and where.
#[test]
fn faults_say_what_and_where() {
    let e = eval("tool_diameter * spindle").unwrap_err();
    assert_eq!(e.at, 17);
    assert!(e.message.contains("spindle"), "{e}");
    assert!(eval("tool_diameter *").is_err());
    assert!(eval("(1 + 2").unwrap_err().message.contains(')'));
    assert!(eval("1 / 0").unwrap_err().message.contains("zero"));
    assert!(eval("Math.sqrt(-1)").is_err());
    assert!(
        eval("Math.nope(1)")
            .unwrap_err()
            .message
            .contains("Math.nope")
    );
    assert!(eval("tool_type").is_err(), "text is no number");
    assert!(eval("'open").is_err());
    assert!(eval("1 2").is_err());
    assert!(eval("").is_err());
}
