# frozen_string_literal: true

require "fileutils"
require "json"
require "minitest/autorun"
require "open3"
require "rbconfig"
require "tmpdir"

# Exercise the generator as a subprocess against both Ruby package names.
class GoldenGenerateTest < Minitest::Test
  def setup
    @root = Dir.mktmpdir("golden-runtime")
    @source = "pie\n  \"a\": 1\n"
    @output = File.join(@root, "output")
    @corpus = File.join(@root, "spec", "mermaid")
    FileUtils.mkdir_p(File.join(@root, "lib"))
    FileUtils.mkdir_p(File.join(@corpus, "pie"))
    File.write(File.join(@root, "Gemfile"), "")
    File.write(File.join(@corpus, "pie", "sample.mmd"), @source)
  end

  def teardown
    FileUtils.remove_entry(@root)
  end

  %w[Etassia Sirena].each do |namespace|
    define_method("test_generates_goldens_with_#{namespace.downcase}") do
      write_runtime(namespace)
      generate
      assert_goldens
    end
  end

  private

  def write_runtime(namespace)
    File.write(File.join(@root, "lib", "#{namespace.downcase}.rb"), <<~RUBY)
      module #{namespace}
        class Engine
          def render(source, today:)
            raise "wrong date" unless today == Date.new(2026, 1, 1)
            "<svg>" + source + "</svg>"
          end
        end
      end
    RUBY
  end

  def generate
    stdout, stderr, status = Open3.capture3(
      { "BUNDLE_GEMFILE" => File.join(@root, "Gemfile"),
        "SIRENA_CORPUS" => @corpus, "GOLDEN_OUT" => @output },
      RbConfig.ruby, File.expand_path("golden_generate.rb", __dir__), chdir: @root
    )
    assert status.success?, "#{stdout}\n#{stderr}"
  end

  def assert_goldens
    assert_equal "<svg>#{@source}</svg>", File.binread(File.join(@output, "pie", "sample.svg"))
    assert_equal @source, File.binread(File.join(@output, "pie", "sample.mmd"))
    assert_manifest
  end

  def assert_manifest
    manifest = JSON.parse(File.read(File.join(@output, "manifest.json")))
    assert_equal "2026-01-01", manifest.fetch("today")
    assert_equal [], manifest.fetch("errors")
    assert_equal(["pie/sample.mmd"], manifest.fetch("cases").map { |entry| entry.fetch("case") })
  end
end
