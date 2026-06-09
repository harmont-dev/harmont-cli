# Ruby example

Tiny Gem with RSpec + Rubocop. Pipeline shares one bundler install across rspec + rubocop. Run `bundle install` once locally to generate `Gemfile.lock` (commit it).

## Run the pipeline

```sh
cd examples/ruby
hm run ci --local
```

See `.hm/pipeline.py` for the definition; `examples/README.md` for the full index.
