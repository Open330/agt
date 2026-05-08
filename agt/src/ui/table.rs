use comfy_table::{Cell, ContentArrangement, Table};
use comfy_table::presets::UTF8_HORIZONTAL_ONLY;

pub fn new_table() -> Table {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.load_preset(UTF8_HORIZONTAL_ONLY);
    table
}

pub fn add_row(table: &mut Table, cells: &[&str]) {
    let cells: Vec<Cell> = cells.iter().map(|s| Cell::new(*s)).collect();
    table.add_row(cells);
}

pub fn add_row_cells(table: &mut Table, cells: Vec<Cell>) {
    table.add_row(cells);
}
