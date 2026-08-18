// A conference-paper shape: two columns, figures with captions, a table, a
// bibliography, cross-references, and math. The things the synthetic `#lorem`
// corpus left out.
#set document(title: "On the Measurement of Things", author: "A. Author")
#set page(paper: "a4", columns: 2, margin: 2cm, numbering: "1")
#set text(font: "Libertinus Serif", size: 10pt)
#set heading(numbering: "1.1")
#set math.equation(numbering: "(1)")
#show heading: set block(above: 1.4em, below: 0.8em)

#place(top + center, scope: "parent", float: true)[
  #text(17pt)[*On the Measurement of Things*]

  #v(0.6em)
  A. Author, B. Author \
  #text(8pt)[Institute of Applied Measurement]

  #v(0.8em)
  #par(justify: false)[
    *Abstract.* #lorem(70)
  ]
]

= Introduction <intro>

#lorem(120) See @method for the approach and @results for what it produced.

#lorem(90)

= Method <method>

#lorem(60)

$ E = integral_0^oo (partial f)/(partial x) dif x + sum_(i=1)^n alpha_i beta^i $

#lorem(80) The derivation follows @eq-main.

$ cal(L)(theta) = -1/N sum_(i=1)^N [y_i log hat(y)_i + (1 - y_i) log (1 - hat(y)_i)] $ <eq-main>

#lorem(70)

#figure(
  rect(width: 100%, height: 3cm, fill: luma(240), stroke: 0.5pt)[
    #align(center + horizon)[#text(fill: luma(120))[Figure placeholder]]
  ],
  caption: [A schematic of the apparatus.],
) <fig-apparatus>

#lorem(60) As shown in @fig-apparatus, the arrangement is straightforward.

= Results <results>

#lorem(50)

#figure(
  table(
    columns: (auto, 1fr, 1fr, 1fr),
    align: (left, right, right, right),
    stroke: 0.4pt,
    table.header([*Condition*], [*Mean*], [*SD*], [*n*]),
    [Baseline], [12.4], [1.2], [40],
    [Treatment A], [18.9], [2.1], [38],
    [Treatment B], [17.2], [1.8], [41],
    [Treatment C], [21.5], [3.4], [39],
    [Combined], [17.5], [2.4], [158],
  ),
  caption: [Summary statistics across conditions.],
) <tbl-results>

#lorem(90) @tbl-results reports the aggregate.

#lorem(110)

= Discussion

#lorem(140)

- #lorem(20)
- #lorem(25)
- #lorem(18)

#lorem(100)

= Conclusion

#lorem(70)

#bibliography("refs.bib", style: "ieee")
