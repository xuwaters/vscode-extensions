// Vector-heavy, standing in for a CeTZ or Fletcher document. The package
// registry is not reachable from the benchmark, so the shapes are drawn with
// the standard library — the point is thousands of vector elements per page,
// which is what makes those packages expensive.
#set page(paper: "a4", margin: 1.5cm)
#set text(font: "Libertinus Serif", size: 10pt)

#let scatter(seed, count) = {
  let points = ()
  let x = seed
  for i in range(count) {
    // A cheap deterministic PRNG: no randomness available at compile time.
    x = calc.rem(x * 1103515245 + 12345, 2147483648)
    let px = calc.rem(x, 1000) / 1000
    x = calc.rem(x * 1103515245 + 12345, 2147483648)
    let py = calc.rem(x, 1000) / 1000
    points.push((px * 100%, py * 100%))
  }

  box(width: 100%, height: 7cm, stroke: 0.5pt, inset: 4pt)[
    #for p in points {
      place(dx: p.at(0), dy: p.at(1), circle(radius: 1.5pt, fill: rgb("#3b6ea5")))
    }
  ]
}

#let grid-lines(n) = {
  box(width: 100%, height: 5cm, stroke: 0.5pt)[
    #for i in range(n) {
      place(dx: i / n * 100%, line(end: (0%, 5cm), stroke: 0.3pt + luma(180)))
      place(dy: i / n * 100%, line(end: (100%, 0%), stroke: 0.3pt + luma(180)))
    }
  ]
}

#for chart in range(1, 9) {
  heading(level: 2)[Figure #chart]
  scatter(chart * 7919, 400)
  grid-lines(40)
  lorem(40)
  pagebreak(weak: true)
}
