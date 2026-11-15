use comfy_table::{Cell, Table, presets::UTF8_FULL};
use spdk::env::{cpu_cores, numa_nodes};

#[spdk::main]
async fn main() {
    let mut numa_table = Table::new();

    numa_table
        .load_style(UTF8_FULL)
        .set_header(vec!["NUMA Node", "CPU Mask"]);

    numa_nodes().for_each(|node| {
        numa_table.add_row(vec![Cell::new(node), Cell::new(node.cpuset())]);
    });

    println!("NUMA Information:");
    println!("{numa_table}");
    println!();

    let mut cpu_table = Table::new();

    cpu_table
        .load_style(UTF8_FULL)
        .set_header(vec!["CPU", "SMT Mask"]);

    cpu_cores().for_each(|core| {
        cpu_table.add_row(vec![
            Cell::new(core.id()),
            Cell::new(core.siblings().unwrap_or_default()),
        ]);
    });

    println!("CPU Information:");
    println!("{cpu_table}");
    println!();
}
