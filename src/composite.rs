//! Text laid out in layers, for the charts: each layer is a string placed
//! at a column with a color, and later layers cover earlier ones.

use crate::color::Color;
use crate::text::char_width;

#[derive(Default)]
pub struct Composite {
    layers: Vec<(String, usize, Color)>,
}

#[derive(Clone, Copy)]
enum Cell {
    /// Nothing here (shown as a space without color).
    Empty,
    /// A character of a layer.
    Char(usize, char),
    /// A space owned by a layer: the second half of a wide character, or
    /// the visible half of a half-covered one.
    Pad(usize),
}

impl Composite {
    pub fn add(&mut self, text: impl Into<String>, offset: usize, color: Color) {
        self.layers.push((text.into(), offset, color));
    }

    pub fn render(&self) -> String {
        let mut cells: Vec<Cell> = Vec::new();
        let put = |cells: &mut Vec<Cell>, i: usize, cell: Cell| {
            if cells.len() <= i {
                cells.resize(i + 1, Cell::Empty);
            }
            cells[i] = cell;
        };
        for (layer, (text, offset, _)) in self.layers.iter().enumerate() {
            let mut col = *offset;
            let mut first = true;
            for c in text.chars() {
                let w = char_width(c);
                if w == 0 {
                    continue;
                }
                if first && col > 0 {
                    // A wide character whose second half is covered now.
                    if let Some(Cell::Char(l, prev)) = cells.get(col - 1).copied()
                        && char_width(prev) == 2
                    {
                        cells[col - 1] = Cell::Pad(l);
                    }
                }
                first = false;
                put(&mut cells, col, Cell::Char(layer, c));
                if w == 2 {
                    put(&mut cells, col + 1, Cell::Pad(layer));
                }
                col += w;
            }
        }

        let mut out = String::new();
        let mut current: Option<usize> = None;
        let mut i = 0;
        while i < cells.len() {
            let (layer, ch, width) = match cells[i] {
                Cell::Empty => (None, ' ', 1),
                Cell::Char(l, c) => (Some(l), c, char_width(c)),
                Cell::Pad(l) => (Some(l), ' ', 1),
            };
            if layer != current {
                if let Some(prev) = current {
                    out.push_str(self.layers[prev].2.end());
                }
                if let Some(l) = layer {
                    out.push_str(&self.layers[l].2.code());
                }
                current = layer;
            }
            out.push(ch);
            i += width;
        }
        if let Some(prev) = current {
            out.push_str(self.layers[prev].2.end());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_cover() {
        let mut c = Composite::default();
        c.add("aaaaaaaaaa", 2, Color::none());
        c.add("bbbbb", 5, Color::none());
        c.add("c", 15, Color::none());
        assert_eq!(c.render(), "  aaabbbbbaa   c");
    }
}
