//! Generates `src/services/bindings.ts` from the Rust contract.
//! `--check` verifies the committed file is up to date (CI / pre-commit).

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const ROOT = path.resolve(import.meta.dirname, '..')
const BINDINGS = path.join(ROOT, 'src/services/bindings.ts')

const args = process.argv.slice(2)
if (args.length > 1 || (args.length === 1 && args[0] !== '--check')) {
  console.error('usage: gen-contract.mjs [--check]')
  process.exit(2)
}
const check = args[0] === '--check'

// The scratch directory lives inside the repo on purpose: on Windows the
// process temp directory can be unwritable for a sandboxed build, and a
// relative path keeps `--check` working everywhere.
const scratchRoot = path.join(ROOT, 'src-tauri', 'target', 'contract-check')
fs.mkdirSync(scratchRoot, { recursive: true })
const tmpDir = check ? fs.mkdtempSync(path.join(scratchRoot, 'run-')) : null
const output = tmpDir ? path.join(tmpDir, 'bindings.ts') : BINDINGS

const env = {
  ...process.env,
  PATH: `${process.env.USERPROFILE ?? ''}\\.cargo\\bin;${process.env.PATH ?? ''}`,
}

try {
  execFileSync(
    'cargo',
    ['run', '--quiet', '--bin', 'x-matrix', '--', `--export-contract=${output}`],
    { cwd: path.join(ROOT, 'src-tauri'), stdio: 'inherit', env },
  )

  if (!fs.existsSync(output)) {
    console.error('contract export produced no file')
    process.exit(1)
  }

  fs.mkdirSync(path.dirname(BINDINGS), { recursive: true })

  if (check) {
    const same = fs.readFileSync(output, 'utf8') === fs.readFileSync(BINDINGS, 'utf8')
    if (!same) {
      console.error('IPC bindings are out of date. Run `npm run gen:contract` and commit the result.')
      process.exitCode = 1
    } else {
      console.log('contract up to date')
    }
  } else {
    console.log(`contract written: ${path.relative(ROOT, BINDINGS)}`)
  }
} finally {
  if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true })
}
