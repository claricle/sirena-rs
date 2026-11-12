# frozen_string_literal: true

require "minitest/autorun"
require "tmpdir"
require "fileutils"
require "open3"
require "json"
require "digest"
require "rbconfig"

class GoldenGenerateTest < Minitest::Test
  def test_generates_goldens_from_the_etassia_checkout
    Dir.mktmpdir("etassia-golden-") do |checkout|
      FileUtils.mkdir_p(File.join(checkout, "lib"))
      FileUtils.mkdir_p(File.join(checkout, "spec/mermaid/pie"))
      File.write(File.join(checkout, "lib/etassia.rb"), <<~RUBY)
        module Etassia
          class Engine
            def render(source, today:)
              raise "unexpected date" unless today == Date.new(2026, 1, 1)
              "<svg>" + source + "</svg>"
            end
          end
        end
      RUBY
      source = "pie\n  \"a\": 1\n"
      File.binwrite(File.join(checkout, "spec/mermaid/pie/sample.mmd"), source)
      output = File.join(checkout, "golden")
      script = File.expand_path("../golden_generate.rb", __dir__)
      stdout, stderr, status = Open3.capture3({ "GOLDEN_OUT" => output, "SIRENA_CORPUS" => nil },
                                             RbConfig.ruby, script, chdir: checkout)

      assert status.success?, "#{stdout}\n#{stderr}"
      manifest = JSON.parse(File.read(File.join(output, "manifest.json")))
      assert_equal [], manifest.fetch("errors")
      svg = "<svg>#{source}</svg>"
      assert_equal [{ "case" => "pie/sample.mmd", "type" => "pie",
                      "source_sha256" => Digest::SHA256.hexdigest(source),
                      "svg_sha256" => Digest::SHA256.hexdigest(svg) }], manifest.fetch("cases")
      assert_equal source, File.binread(File.join(output, "pie/sample.mmd"))
      assert_equal svg, File.binread(File.join(output, "pie/sample.svg"))
    end
  end
end
