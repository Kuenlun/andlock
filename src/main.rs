/*!
andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
Copyright (C) 2026  Juan Luis Leal Contreras (Kuenlun)

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

// Allow `#[coverage(off)]` on test modules under `--cfg coverage_nightly` (nightly-only).
#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

mod cli;
mod json_format;
mod memory;
mod output;
mod pipeline;
mod preview;
mod tty;

fn main() -> anyhow::Result<()> {
    tty::install_handler()?;
    cli::run()
}
