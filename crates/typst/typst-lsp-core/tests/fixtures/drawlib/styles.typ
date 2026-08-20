// A style dictionary keyed by drawing root, in the shape cetz keeps one: the
// global defaults at the top, then one sub-dictionary per root naming exactly
// the keys that root accepts.
#let default = (
  fill: none,
  stroke: (paint: black, thickness: 1pt),
  circle: (
    radius: auto,
    stroke: auto,
    fill: auto,
  ),
)

#let resolve(root: none, merge: (:)) = {
  default.at(root, default: (:)) + merge
}
