// A presentation: many small pages, heavy per-page styling, images and lists.
#set page(paper: "presentation-16-9", margin: 2cm, fill: rgb("#fdfdfd"))
#set text(font: "Libertinus Serif", size: 22pt)

#let slide(title, body) = {
  page[
    #text(30pt, weight: "bold")[#title]
    #v(0.5em)
    #line(length: 100%, stroke: 1.5pt + rgb("#3b6ea5"))
    #v(0.8em)
    #body
  ]
}

#for n in range(1, 31) {
  slide[Topic #n][
    #lorem(24)

    - #lorem(8)
    - #lorem(10)
    - #lorem(7)

    #align(center)[
      #rect(width: 60%, height: 3cm, radius: 4pt, fill: rgb("#eef3f8"), stroke: 1pt + rgb("#3b6ea5"))[
        #align(center + horizon)[#text(16pt)[Diagram #n]]
      ]
    ]
  ]
}
