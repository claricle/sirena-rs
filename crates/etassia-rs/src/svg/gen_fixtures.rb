# Emits the Ruby-computed expectations embedded in tests/svg_parity.rs.
# Usage: cd <sirena ruby checkout> && bundle exec ruby <this file> > fixtures.json
require "json"
require "sirena"

nums = [0.0, -0.0, 1.0, -1.0, 10.0, 0.5, 0.1, 0.2 + 0.1, 12.0 * 1.1, 1.0 / 3, 2.0 / 3, 100.0,
        123.456, 1234.5678, 0.00001234, 1e-5, 1e-6, 1e-7, 1.5e-7, 0.000123, 1e15, 1e16, 1e17,
        123456789012345678.0, 1e20, 1e21, 1.5e21, 1.234e25, 9007199254740993.0, 2.0**53, 2.0**63,
        -1e21, -0.00001, -123.456, 5e-324, 1.7976931348623157e308, 0.0001, 0.001, 99999.99999,
        13.200000000000001, 0.30000000000000004, 3.14159265358979, 1e22, 1e100, 1.0e-10,
        0.00004999, 0.00005, 0.00015, 1.00005, 2.5, 2.675, 1.23455, 1.23445, -2.00005, 8.5e-5,
        123456.78915, 17592186044416.0, 17592186044415.99996, 1e300, -1e-300, 4503599627370496.5]
rng = Random.new(343)
200.times { nums << (rng.rand - 0.5) * 10**rng.rand(-8..12) }
200.times { nums << rng.rand(0..100_000) / 100.0 }
nums.uniq!
numbers = nums.map do |n|
  { "v" => n, "js" => Sirena::JsNumber.stringify(n), "ruby" => n.to_s, "round4" => n.round(4),
    "write" => Sirena::Svg::Numbers.write(n) }
end

S = Sirena::Svg
build = lambda do |spec|
  klass = S.const_get(spec["tag"] == "g" ? "Group" : spec["tag"].capitalize)
  obj = klass.new
  (spec["attrs"] || []).each do |k, v|
    obj.public_send("#{k == 'class' ? 'class_name' : k.tr('-', '_')}=", v)
  end
  case spec["tag"]
  when "g" then (spec["children"] || []).each { |c| obj << build.call(c) }
  when "text"
    obj.content = [spec["text"]] if spec["text"]
    obj.tspans = (spec["children"] || []).map { |c| build.call(c) }
  when "tspan"
    obj.content = [spec["text"]] if spec["text"]
    obj.line_shift = spec["line_shift"] if spec["line_shift"]
  end
  obj
end
SPEC_ATTRS = ->(h) { h.to_a }
trees = [
  { "tag" => "rect", "attrs" => [["x", 10.0], ["y", 20.5], ["width", 100.0], ["height", 40.0], ["rx", 5.0],
                                 ["fill", "#fff"], ["stroke-width", "2"], ["id", "a<b"], ["class", "n\"q'"]] },
  { "tag" => "rect", "attrs" => [["x", 1e-5], ["y", 1e16], ["width", 3.0], ["height", 4.0],
                                 ["opacity", "0.5"], ["fill-opacity", "0.5"], ["stroke-opacity", "abc"]] },
  { "tag" => "rect", "attrs" => [["x", 0.0], ["y", 0.0], ["width", 1.0], ["height", 1.0], ["opacity", "0.3"]] },
  { "tag" => "circle", "attrs" => [["cx", 5.0], ["cy", 6.25], ["r", 3.0], ["fill", "red"], ["stroke", "#000"]] },
  { "tag" => "ellipse", "attrs" => [["cx", 5.0], ["cy", 6.0], ["rx", 3.0], ["ry", 2.0]] },
  { "tag" => "line", "attrs" => [["x1", 0.0], ["y1", 0.0], ["x2", 9.5], ["y2", 9.0], ["stroke-dasharray", "5,5"]] },
  { "tag" => "polygon", "attrs" => [["points", "0,0 10,0 5,8"], ["fill", "#abc"]] },
  { "tag" => "path", "attrs" => [["d", "M 0 0 L 10 10 Z"], ["stroke-linecap", "round"], ["transform", "translate(1,2)"]] },
  { "tag" => "text", "text" => "a & b <c>", "attrs" => [["x", 10.0], ["y", 20.0], ["text-anchor", "middle"],
                                                    ["font-size", "14"], ["font-family", "Arial"]] },
  { "tag" => "text", "text" => "dx", "attrs" => [["x", 10.0], ["dx", 2.5], ["y", 20.0], ["dy", 3.25],
                                               ["dominant-baseline", "middle"], ["font-size", "20px"]] },
  { "tag" => "text", "text" => "hang", "attrs" => [["dominant-baseline", "Hanging "], ["y", 1.0]] },
  { "tag" => "text", "text" => "line1\nline2", "attrs" => [["x", 5.0], ["y", 10.0], ["font-size", "10"]] },
  { "tag" => "text", "text" => "", "attrs" => [["x", 5.0], ["y", 10.0], ["font-size", "10"]],
    "children" => [{ "tag" => "tspan", "text" => "one", "line_shift" => 0, "attrs" => [["x", 5.0]] },
                   { "tag" => "tspan", "text" => "two <2>", "line_shift" => 1, "attrs" => [["x", 5.0], ["font-weight", "bold"]] },
                   { "tag" => "tspan", "text" => "three", "line_shift" => 2, "attrs" => [["x", 5.0]] }] },
  { "tag" => "g", "attrs" => [["transform", "translate(5,5)"], ["opacity", "0.5"]], "children" => [
    { "tag" => "rect", "attrs" => [["x", 1.0], ["y", 2.0], ["width", 3.0], ["height", 4.0]] },
    { "tag" => "g", "attrs" => [], "children" => [] },
    { "tag" => "g", "attrs" => [["id", "inner"]], "children" => [
      { "tag" => "text", "text" => "multi\nline", "attrs" => [["x", 1.0], ["y", 2.0]] },
      { "tag" => "circle", "attrs" => [["cx", 1.0], ["cy", 1.0], ["r", 1.0]] }
    ] }
  ] },
]
structures = trees.map { |t| { "spec" => t, "xml" => build.call(t).to_xml } }

docs = [
  { "width" => 200.0, "height" => 100.0, "view_box" => nil, "overflow" => nil, "children" => trees[0, 3] },
  { "width" => 33.5, "height" => 1e16, "view_box" => "0 0 33 44", "overflow" => "hidden", "children" => trees[12, 2] },
  { "width" => 10.0, "height" => 10.0, "view_box" => nil, "overflow" => nil, "children" => [] },
].map do |d|
  doc = S::Document.new(width: d["width"], height: d["height"], view_box: d["view_box"], overflow: d["overflow"])
  d["children"].each { |c| doc << build.call(c) }
  d.merge("xml" => doc.to_xml)
end

escapes = ["plain", "a&b<c>d\"e'f", "nul\u0000x\u0001y\ttab\nnl\u{1F600}￾퟿"].map do |s|
  { "in" => s, "text" => S::Escaping.escape_text(s), "attr" => S::Escaping.escape_attribute(s) }
end

themes = Sirena::Theme::Registry.list.map do |n|
  t = Sirena::Theme::Registry.get(n)
  { "name" => n.to_s, "background" => t.colors.background, "font_size_normal" => t.typography.font_size_normal,
    "flow3" => t.colors.palette(:flow, 3), "flow11" => t.colors.palette(:flow, 11),
    "shadow_enabled" => t.effects.shadow_enabled, "dash" => t.shapes.dash_pattern_dashed }
end

reads = ["14px", " 12.5", "-3e2x", ".5", "1.", "abc", "", "+7", "1e", "1e+", "0.5e-1"].map do |s|
  { "in" => s, "out" => Sirena::Svg::Numbers.read(s) }
end

puts JSON.generate({ "numbers" => numbers, "structures" => structures, "documents" => docs,
                     "escapes" => escapes, "themes" => themes, "reads" => reads })
