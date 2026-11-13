# frozen_string_literal: true

# Dumps Ruby text-measurement results for the Rust parity test
# (crates/etassia-core/tests/text_parity.rs). Floats are written as their
# IEEE-754 bit patterns (u64) so the comparison is exact.
#
#   cd $ETASSIA && ruby /path/to/etassia-rs/scripts/text_parity_dump.rb
#
# Env: TEXT_PARITY_OUT (fixture path), GOLDEN_DIR (corpus *.mmd source).
$LOAD_PATH.unshift(File.join(Dir.pwd, "lib"))
require "json"
require "etassia/text_measurement"
require "etassia/text_measurement/times_advances"
require "etassia/text_measurement/trebuchet_kerning"
require "etassia/layout/timeline_text"

ROOT = File.expand_path("..", __dir__)
OUT = ENV.fetch("TEXT_PARITY_OUT",
                File.join(ROOT, "crates/etassia-core/tests/fixtures/text/parity.json"))
GOLDEN = ENV.fetch("GOLDEN_DIR", File.join(ROOT, "golden"))
TM = Etassia::TextMeasurement
TL = Etassia::Layout::TimelineText
SIZES = [10.0, 14.5, 16.0, 24.0].freeze
WRAP_WIDTH = 120.0

def bits(float) = [float.to_f].pack("G").unpack1("Q>")

lines = Dir.glob(File.join(GOLDEN, "*", "*.mmd")).sort.flat_map do |path|
  File.read(path, encoding: "UTF-8").scrub.lines.map { |l| l.chomp.strip }
end
strings = lines.reject(&:empty?).uniq
strings |= strings.flat_map(&:split).uniq

EDGE = [
  "", " ", "\t", "\n", "\r\n", "a\tb\nc\rd", "\u0000", "a\u0000b", "\u007F",
  "\u0001\u001F", "  lead and trail  ", "a   b", "a b",
  "😀", "😀😀", "a😀b", "👨‍👩‍👧‍👦", "❤️", "❤", "©", "©️", "®️", "™️", "#️⃣",
  "♭", "♭️", "☀️", "️", "a️", "🇩🇪", "🏳️‍🌈",
  "中文字符", "日本語のテキスト", "한국어", "ＡＢＣ１２３", "ｶﾀｶﾅ",
  "é", "é", "é̂̃", "​‌‍", "‮", "﻿",
  "ﷺ", "ﷻ﷼﷽", "مرحبا", "שלום", "Ελληνικά", "Кириллица",
  "҇", "‐", " ", "₿", "⇔", "\u{10FFFF}", "\u{E000}",
  "\u{FFFE}", "\u{1F600}\u{FE0F}", "To Pe Ro Re Po A T", " A T", "Tom Peter Robert",
  "Hello, World!", "The quick brown fox jumps over the lazy dog 0123456789",
  "a<br>b", "x <br> y<br>z", "<br>", "a<br/>b", "word " * 40,
  "supercalifragilisticexpialidocious_is_a_very_long_unbreakable_word",
  "tab\there  two  spaces nbsp", "\v\f"
].freeze
strings |= EDGE

def table_cps(table) = table.keys
def range_edges(table)
  table.flat_map { |lo, hi| [lo - 1, lo, hi, hi + 1] }
end

tables = [TM::ArialAdvances::TABLE, TM::TrebuchetAdvances::TABLE,
          TM::TimesAdvances::TABLE]
cps = (0..0x2FFF).to_a + (0x3000..0x10FFFF).step(1009).to_a
cps |= tables.flat_map { |t| table_cps(t) }
cps |= TM::OUTLIERS.keys
# Boundaries of the Unicode classes the Rust tables encode.
[TM::ZERO_WIDTH, TM::EMOJI].each do |re|
  prev = false
  (0..0x10FFFF).each do |cp|
    next if (0xD800..0xDFFF).cover?(cp)

    hit = cp.chr(Encoding::UTF_8).match?(re)
    cps.push(cp - 1, cp) if hit != prev
    prev = hit
  end
end
cps = cps.select { |cp| cp.between?(0, 0x10FFFF) && !(0xD800..0xDFFF).cover?(cp) }.uniq.sort
singles = cps.map { |cp| cp.chr(Encoding::UTF_8) }

def row(text, sizes = SIZES)
  sizes.flat_map do |size|
    arial = TM.measure(text, font_size: size)[:width]
    [bits(arial),
     bits(TM.measure_in(:trebuchet, text, font_size: size)[:width]),
     bits(TM.measure(text, font_size: size, monospace: true)[:width]),
     bits(arial * 1.2),
     bits(TM::TrebuchetKerning.adjustment(text, font_size: size)),
     bits(TL.width_of(text, font_size: size))]
  end
end

cases = strings.map { |t| { "t" => t, "w" => row(t) } }
# One size is enough for the per-codepoint sweep.
sweep = (singles - strings).map { |t| { "t" => t, "w" => row(t, [16.0]) } }
wraps = strings.map do |t|
  wrapped = TL.wrap(t, WRAP_WIDTH)
  { "t" => t, "lines" => wrapped,
    "h" => bits(TL.text_height(wrapped)), "c" => bits(TL.card_height(wrapped)) }
end

File.write(OUT, JSON.generate(
  "ruby" => RUBY_VERSION, "sizes" => SIZES.map { |s| bits(s) },
  "wrap_width" => bits(WRAP_WIDTH), "cases" => cases, "sweep" => sweep, "wraps" => wraps
))
puts "wrote #{OUT}: #{cases.size} strings, #{sweep.size} sweep, #{wraps.size} wraps"
