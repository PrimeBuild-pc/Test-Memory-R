# TODO 132: LPDDR5X in the transfer-width docs and `beat=`

Raised by the owner, 2026-11-22, from forum feedback: "LPDDR5X is missing between the LPDDR4/5 description" (the README's
transfer-width table).

## What TMR does today

- `switch`'s transfer width comes from SMBIOS Type 17's memory type (`pattern_gen.rs`, `BusBeat::from_smbios_type`): 0x1E LPDDR4
  and 0x23 LPDDR5 give 16 bits.
- SMBIOS has no LPDDR5X code. dmidecode's table (checked 2026-11-22) ends at 0x24 HBM3, so an LPDDR5X system reports 0x23,
  LPDDR5. LPDDR5X keeps LPDDR5's 16-bit channels, so TMR already picks the right width on one. No test machine needed for that.

## The change

- README's transfer-width table and `doc/pattern_spec.md`: "LPDDR4, LPDDR5, LPDDR5X | 16-bit", with a note that SMBIOS
  reports LPDDR5X as LPDDR5.
- `beat=lpddr5x` as a spelling of 16 (`pattern.rs`'s `parse_beat`), and in its help and error text.
- `smbios::memory_type_name`'s 0x23 named "LPDDR5 (or LPDDR5X)" where the system table shows it, if that reads better than
  "LPDDR5" on an LPDDR5X laptop.

## Next

Make the change (small), a test for `beat=lpddr5x`.
