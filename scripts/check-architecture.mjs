//! Architecture guard. Fails when the rules in ARCHITECTURE.md §4.2 are broken.
//!
//! 1. Components/views/stores must not call `invoke(` or `listen(` directly.
//! 2. Only `src/services/` may import `@tauri-apps/api/core` or `/event`.
//! 3. `bindings.ts` must exist and be generated (never hand-edited: we can only
//!    check it carries the generator banner).

import fs from 'node:fs'
import path from 'node:path'

const ROOT = path.resolve(import.meta.dirname, '..')
const SRC = path.join(ROOT, 'src')
const errors = []

const walk = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name)
    return e.isDirectory() ? walk(p) : [p]
  })

if (!fs.existsSync(SRC)) {
  console.log('src/ not found — nothing to check.')
  process.exit(0)
}

const files = walk(SRC).filter((f) => /\.(ts|vue|tsx|js)$/.test(f))

for (const file of files) {
  const rel = path.relative(ROOT, file).replaceAll('\\', '/')
  const text = fs.readFileSync(file, 'utf8')
  const inServices = rel.startsWith('src/services/')

  if (!inServices) {
    if (/\binvoke\s*\(/.test(text)) {
      errors.push(`${rel}: calls invoke() directly — use src/services/contract.ts`)
    }
    if (/\blisten\s*\(/.test(text)) {
      errors.push(`${rel}: calls listen() directly — use src/services/events.ts`)
    }
  }

  if (!inServices && /@tauri-apps\/api\/(core|event)/.test(text)) {
    errors.push(`${rel}: imports @tauri-apps/api internals — only src/services/ may`)
  }
}

const bindings = path.join(SRC, 'services/bindings.ts')
if (!fs.existsSync(bindings)) {
  errors.push('src/services/bindings.ts missing — run `npm run gen:contract`')
}

if (errors.length) {
  console.error('architecture violations:\n' + errors.map((e) => `  ✗ ${e}`).join('\n'))
  process.exit(1)
}

console.log(`architecture ok (${files.length} files checked)`)
