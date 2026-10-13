import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, existsSync } from 'node:fs';

// Defense-in-depth, not a guarantee. Review new assets and history separately.
// A local source ZIP has no Git metadata but must pass the same privacy scan.
function sourceFiles(root = '.') {
  return readdirSync(root, { withFileTypes: true }).flatMap(entry => {
    if (['.git', 'node_modules', 'target', 'dist', 'coverage', 'artifacts', '.wrangler', '.DS_Store'].includes(entry.name)) return [];
    const path = root === '.' ? entry.name : `${root}/${entry.name}`;
    if (entry.isSymbolicLink()) throw new Error(`Refusing linked source: ${path}`);
    return entry.isDirectory() ? sourceFiles(path) : [path];
  });
}
const paths = existsSync('.git')
  ? [...new Set(execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'], {encoding:'utf8'}).split('\0').filter(Boolean))]
  : sourceFiles();
const failures = [];
const forbiddenFile = /(^|\/)(auth\.json|cloud-sync\.json|usage-sync\.json|local-usage\.json|local-usage-cache\.json|cloud-snapshots|\.dev\.vars[^/]*|wrangler\.local\.[^/]+|\.env(?!\.example)[^/]*|id_rsa|id_ed25519)(\/|$)|\.(sqlite3?|db|p12|pfx|pem|key)$/i;
for (const path of paths) {
  if (forbiddenFile.test(path)) failures.push(`${path}: forbidden runtime/credential file`);
  const data = readFileSync(path);
  if (data.includes(0) || /\.(png|ico|icns|woff2?)$/i.test(path)) continue;
  const lines = data.toString('utf8').split('\n');
  for (const [index, line] of lines.entries()) {
    const checks = [
      ['private key', /-----BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY-----/],
      ['access token', /\b(?:ghp_|github_pat_|sk-proj-)[A-Za-z0-9_]{20,}/],
      ['deployment database id', /^\s*database_id\s*=\s*"[a-f0-9-]{32,}"/i],
      ['personal home path', /(?:\/Users\/(?!test(?:\/|\b)|example(?:\/|\b)|user(?:\/|\b)|runner(?:\/|\b))[A-Za-z0-9._-]+\/|C:\\\\Users\\\\(?!test|example|user|runner)[A-Za-z0-9._-]+\\\\)/i],
    ];
    for (const [kind, pattern] of checks) if (pattern.test(line)) failures.push(`${path}:${index + 1}: ${kind}`);
  }
}
if (failures.length) { console.error(failures.join('\n')); process.exitCode = 1; }
else console.log(`Publication audit passed (${paths.length} source files; manual review still required).`);
