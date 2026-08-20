// A drawing library documented the way the Typst ecosystem documents one:
// tidy-style doc comments, real arguments taken through a sink and read back
// out of a style dictionary. Modelled on cetz, small enough to read.
#import "styles.typ"

/// Draws a circle or ellipse.
///
/// ```example
/// circle((0, 0))
/// ```
///
/// - ..points-style (coordinate, style): The position to place the circle on.
///   If given two coordinates, the distance between them is the radius.
/// - name (none, str): A name for the element, to anchor other elements to.
/// - anchor (none, str):
///
/// === Styling
/// *Root*: `circle`
///
/// - radius (number, array) = 1: The size of the circle's radius.
///
/// === Anchors
///   Supports border anchors.
#let circle(..points-style, name: none, anchor: none) = {
  let style = styles.resolve(root: "circle", merge: points-style.named())
  none
}

/// Draws an ellipse through two points.
///
/// The styling section names a root and documents no keys of its own, which
/// is how a library says "the same styling as `circle`".
///
/// - a (coordinate): One point.
/// - b (coordinate): The other.
///
/// == Styling
/// *Root:* `circle`
#let ellipse-through(a, b, ..style) = {
  let style = styles.resolve(root: "circle", merge: style.named())
  none
}

/// Places a label. Takes no sink, so it accepts exactly what it declares.
///
/// - position (coordinate): Where to put it.
/// - text (str): What to write.
#let label-at(position, text: "") = none
