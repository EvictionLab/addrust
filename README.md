# addrust

A fast, configurable address parser for US addresses, built in Rust.

addrust breaks addresses into structured components using a pipeline of **rewrite** steps (standardize text in place) and **extract** steps (pull components into fields). The pipeline is configured in TOML — steps can be reordered, customized, or extended without modifying source code. Domain knowledge (suffixes, directionals, abbreviations) lives in lookup tables, not code.

## Quick start

```sh
cargo install --path .
echo "123 N Main St Apt 4, Springfield IL 62704" | addrust parse --format full
```

## Output fields

| Field | Description | Example |
|-------|-------------|---------|
| `street_number` | House/building number | 123 |
| `pre_direction` | Directional before street name | N, SW |
| `street_name` | Street name | MAIN |
| `suffix` | Street type | STREET, AVENUE |
| `post_direction` | Directional after street name | NW |
| `unit_type` | Unit designator | APT, STE, FL |
| `unit` | Unit number/identifier | 4, B |
| `po_box` | PO Box number | 1234 |
| `building` | Building name or number | |

## Usage

### DuckDB

Most common workflow — parse addresses directly from a DuckDB table:

```sh
cargo install --path .
```

```sh
addrust parse --duckdb mydata.duckdb --input-table raw_addresses
```

This creates a `raw_addresses_parsed` table with one column per output field. Additional options:

```sh
addrust parse \
  --duckdb mydata.duckdb \
  --input-table raw_addresses \
  --output-table parsed \
  --column addr_field \
  --overwrite \
  --config project.toml
```

### stdin

For quick parsing without a database, pipe in a text file with one address per line:

```sh
cat addresses.txt | addrust parse --format tsv
```

### Inspecting the pipeline

```sh
addrust list steps              # all steps with enabled/disabled status
addrust list tables             # all dictionary tables
addrust list tables suffix_all  # entries in a specific table
```

## Configuration

addrust works out of the box with no configuration. To customize, create a `.addrust.toml` in your working directory:

```sh
addrust init                    # generate a starter config
addrust configure               # interactive TUI editor (press s to save)
```

Use a config from a different location:

```sh
addrust --config path/to/config.toml parse
```

### Disable or reorder steps

```toml
[steps]
disabled = ["po_box", "ordinal_to_word"]
step_order = ["na_check", "city_state_zip", "suffix_common", "po_box"]
```

Steps not listed in `step_order` are appended in their default order.

### Add custom steps

```toml
[[steps.custom_steps]]
type = "extract"
label = "my_custom_step"
pattern = '\bBOX (\d+)'
target = "po_box"
skip_if_filled = true
```

### Override dictionary tables

Add entries, add variants to existing entries, or remove entries from any built-in table:

```toml
[dictionaries.suffix_all]
add = [
    { short = "PSGE", long = "PASSAGE" },
]
remove = ["TRAILER"]

[dictionaries.unit_type]
add = [
    { short = "WH", long = "WAREHOUSE", variants = ["WHSE"] },
]
```

### Output format

Control whether parsed components use their short or long form:

```toml
[output]
suffix = "short"       # default: "long" (STREET vs ST)
direction = "long"     # default: "short" (NORTH vs N)
```

Available fields: `suffix`, `direction`, `unit_type`, `unit_location`, `state`.

## Learn more

See the [wiki](https://github.com/EvictionLab/addrust/wiki) for detailed documentation on how the pipeline works, writing custom steps, and dictionary table reference.

## License

[MIT](LICENSE)
