// A long book: many chapters, an outline, running headers, and enough pages to
// exercise the cost of a document that does not fit in a viewport.
#set document(title: "A Long Book")
#set page(
  paper: "a5",
  margin: (inside: 2cm, outside: 1.5cm, y: 2cm),
  numbering: "1",
  header: context {
    if calc.even(here().page()) [#emph[A Long Book]] else [#h(1fr) #emph[Chapter]]
  },
)
#set text(font: "Libertinus Serif", size: 10pt)
#set par(justify: true, first-line-indent: 1.2em)
#set heading(numbering: "1.")

#outline(depth: 2)
#pagebreak()

#let chapter(n) = {
  heading(level: 1)[Chapter #n]
  for section in range(4) {
    heading(level: 2)[Section #n.#section]
    lorem(190)
    parbreak()
    lorem(150)
    parbreak()
  }
}

#for n in range(1, 26) { chapter(n) }
