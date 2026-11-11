import { readFileSync, readdirSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseDocument } from 'yaml';

const directory = join(dirname(fileURLToPath(import.meta.url)), '../.github/workflows');
const snippets = join(directory, '../../target/workflow-scripts');
mkdirSync(snippets, { recursive: true });
for (const name of readdirSync(directory).filter(value => /\.ya?ml$/.test(value))) {
  const document = parseDocument(readFileSync(join(directory, name), 'utf8'), { uniqueKeys: true });
  if (document.errors.length) throw new Error(`${name}: ${document.errors.map(value => value.message).join('; ')}`);
  const workflow = document.toJS();
  if (!workflow.on || !workflow.jobs || !workflow.permissions) throw new Error(`${name}: missing triggers, jobs or explicit permissions`);
  if (name !== 'validation.yml' && Object.keys(workflow.on).some(value => value !== 'workflow_dispatch')) throw new Error(`${name}: releases must be manually dispatched`);
  for (const [jobName, job] of Object.entries(workflow.jobs)) {
    for (const [index, step] of (job.steps ?? []).entries()) {
      if (step.uses && !/^[\w-]+\/[\w-]+@[a-f0-9]{40}$/.test(step.uses)) throw new Error(`${name}: pin action revisions`);
      if (step.run && /\$\{\{\s*(?:inputs\.|github\.event\.)/.test(step.run)) throw new Error(`${name}: pass user input through environment variables, not shell source`);
      if (step.run) {
        if ((step.shell ?? job.defaults?.run?.shell) !== 'pwsh') throw new Error(`${name}: expected an explicit PowerShell shell`);
        writeFileSync(join(snippets, `${name}-${jobName}-${index}.ps1`), step.run);
      }
    }
  }
}
console.log('Workflow YAML and release boundaries passed.');
