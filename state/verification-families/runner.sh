#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export VF_BASE_DIR="$SCRIPT_DIR"

exec /usr/bin/ruby - "$@" <<'RUBY'
require "open3"
require "shellwords"
require "time"
require "yaml"

def load_yaml(path)
  YAML.load_file(path) || {}
rescue Errno::ENOENT
  warn "missing file: #{path}"
  exit 66
end

def apply_template(parts, values)
  Array(parts).map do |part|
    rendered = String(part)
    values.each do |key, value|
      rendered = rendered.gsub("%{#{key}}", value.to_s)
    end
    rendered
  end.reject(&:empty?)
end

base_dir = ENV.fetch("VF_BASE_DIR")
repo_root = File.expand_path("../..", base_dir)
packs_path = File.join(base_dir, "packs.yaml")
packs_doc = load_yaml(packs_path)
packs = packs_doc.fetch("packs")
pack_name = ARGV[0] || "smoke"

unless packs.key?(pack_name)
  warn "usage: #{File.basename($0)} [#{packs.keys.join('|')}]"
  exit 64
end

pack = packs.fetch(pack_name)
started_at = Time.now
family_results = []
declared_tests = 0
executed_commands = 0

pack.fetch("families").each do |family_entry|
  family_id = family_entry.fetch("id")
  intent_path = File.join(base_dir, family_id, "intent.yaml")
  intent = load_yaml(intent_path)
  profile_name = family_entry["profile"] || intent.fetch("profile")
  profile_path = File.join(base_dir, "profiles", "#{profile_name}.yaml")
  profile = load_yaml(profile_path)

  template = profile.dig("command", "template")
  if Array(template).empty?
    warn "profile #{profile_name} has no command template"
    exit 65
  end

  package = family_entry["package"] || intent["package"] || profile.dig("command", "package") || "aterm-core"
  target = family_entry["target"] || intent["target"] || profile.dig("command", "target") || ""
  selectors = Array(intent["selectors"])
  tests = Array(intent["tests"])
  declared_tests += tests.size

  family_started_at = Time.now
  commands = []
  family_passed = true

  selectors.each do |selector|
    argv = apply_template(
      template,
      {
        "package" => package,
        "target" => target,
        "selector" => selector
      }
    )

    stdout, stderr, status = Open3.capture3(*argv, chdir: repo_root)
    output = [stdout, stderr].reject(&:empty?).join

    commands << {
      "argv" => argv,
      "selector" => selector,
      "status" => status.exitstatus,
      "ok" => status.success?,
      "output" => output
    }
    executed_commands += 1
    family_passed &&= status.success?
  end

  family_results << {
    "id" => family_id,
    "feature" => intent["feature"],
    "intent" => intent["intent"].to_s.gsub(/\s+/, " ").strip,
    "profile" => profile_name,
    "tests" => tests.size,
    "selectors" => selectors,
    "commands" => commands,
    "ok" => family_passed,
    "duration" => Time.now - family_started_at
  }
end

finished_at = Time.now
overall_ok = family_results.all? { |result| result["ok"] }
inventory = packs_doc["inventory"] || {}

puts "# aterm verification pack report"
puts
puts "- Pack: `#{pack_name}`"
puts "- Description: #{pack['description']}"
puts "- Repo: `#{repo_root}`"
puts "- Started: `#{started_at.iso8601}`"
puts "- Completed: `#{finished_at.iso8601}`"
puts "- Families: `#{family_results.size}`"
puts "- Declared tests in intents: `#{declared_tests}`"
puts "- Executed cargo commands: `#{executed_commands}`"
if inventory["requested_total_from_task"] && inventory["verified_total_from_source"]
  enumerated = inventory["enumerated_total_from_state_tests_md"]
  if enumerated
    puts "- Inventory note: task requested `#{inventory['requested_total_from_task']}` tests, `state/tests.md` enumerates `#{enumerated}`, and current source test functions verify `#{inventory['verified_total_from_source']}`."
  else
    puts "- Inventory note: task requested `#{inventory['requested_total_from_task']}` tests, but current source test functions verify `#{inventory['verified_total_from_source']}`."
  end
end
puts "- Result: #{overall_ok ? '`PASS`' : '`FAIL`'}"
puts
puts "## Summary"
puts
puts "| Family | Profile | Tests | Commands | Result | Duration (s) |"
puts "| --- | --- | ---: | ---: | --- | ---: |"
family_results.each do |result|
  puts "| #{result['id']} | #{result['profile']} | #{result['tests']} | #{result['commands'].size} | #{result['ok'] ? 'PASS' : 'FAIL'} | #{format('%.2f', result['duration'])} |"
end
puts
puts "## Details"
puts
family_results.each do |result|
  puts "### #{result['id']}"
  puts
  puts "- Feature: #{result['feature']}"
  puts "- Intent: #{result['intent']}"
  selectors_md = result["selectors"].map { |selector| "`#{selector}`" }.join(", ")
  puts "- Selectors: #{selectors_md}"
  puts "- Result: #{result['ok'] ? '`PASS`' : '`FAIL`'}"
  puts

  result["commands"].each do |command|
    puts "- Command: `#{Shellwords.join(command['argv'])}`"
    puts "- Exit code: `#{command['status']}`"
    next if command["ok"]

    excerpt = command["output"].lines.last(40).join.rstrip
    puts
    puts "```text"
    puts excerpt
    puts "```"
  end

  puts
end

exit(overall_ok ? 0 : 1)
RUBY
