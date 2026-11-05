// Hulp voor de tests: planningen lezen, wijzigen en opslaan met de code van Open Planner Studio (OPS) zelf.
// Draai via `node scripts/ops/ops.mjs <ops-repo> …` (bundelt dit bestand met esbuild tegen `<ops-repo>/src`).
//
//   lees <bestand.ifc>…        Leest elk bestand met readIFC en rekent het door met solveProject. Controleert dat
//                              de lezer elke IfcTask en IfcRelSequence uit de tekst heeft, dat elke ouder bestaat
//                              en dat er geen relatie dubbel is. Geeft per bestand één regel JSON; exit 1 bij een fout.
//   wijzig <in> <uit> <json>   Leest <in>, doet de wijziging zoals de app, rekent door en schrijft <uit> met
//                              writeIFC (met de GlobalIds uit <in>, zoals opslaan in de app).
//
// Wijzigingen (JSON, taken op naam):
//   {"geen": true}                                  alleen opnieuw opslaan
//   {"hernoem": "A", "naar": "B"}
//   {"duur": "A", "dagen": 7}
//   {"wis": "A"}                                    met haar relaties en toewijzingen
//   {"relatie": ["A", "B"], "type": "FINISH_START"} met eventueel "lagDagen": 0.5, "lagMinuten": 150.4 of "p6": true
//   {"verplaats": "A", "onder": "B"}                achteraan bij de kinderen van B; WBS-codes opnieuw
//   {"nieuw": "A", "onder": "B", "zoals": "C"}      een kopie van blad C met een nieuw id en naam A
//   {"baseline": "Naam"}
import { readIFC } from '@/services/ifc/ifcReader';
import { writeIFC } from '@/services/ifc/ifcWriter';
import { solveProject } from '@/engine/scheduler/solveProject';
import { solveInputFor } from '@/engine/scheduler/solveInput';
import { applyWbsNumbering } from '@/utils/wbs';
import { generateId } from '@/utils/id';
import { isLeafTask } from '@/utils/taskHierarchy';
import { shownFinish, shownStart } from '@/utils/taskDates';
import { readFileSync, writeFileSync } from 'node:fs';

type Gelezen = ReturnType<typeof readIFC>;

function taak(p: Gelezen, naam: string) {
  const t = p.tasks.find(x => x.name === naam);
  if (!t) throw new Error(`geen taak '${naam}'`);
  return t;
}

function reken(p: Gelezen) {
  const r = solveProject(solveInputFor(p.project, p.tasks, p.sequences, p.calendar, p.resourceCalendars ?? []));
  return r;
}

function wijzig(p: Gelezen, w: Record<string, any>) {
  if (w.geen) return;
  if (w.hernoem) {
    taak(p, w.hernoem).name = w.naar;
  } else if (w.duur) {
    const t = taak(p, w.duur);
    t.time.scheduleDuration = w.dagen;
    delete t.time.durationMinutes;
  } else if (w.wis) {
    const t = taak(p, w.wis);
    p.tasks = p.tasks.filter(x => x.id !== t.id);
    for (const x of p.tasks) x.childIds = x.childIds.filter(c => c !== t.id);
    p.sequences = p.sequences.filter(s => s.predecessorId !== t.id && s.successorId !== t.id);
    p.assignments = p.assignments.filter(a => a.taskId !== t.id);
    applyWbsNumbering(p.tasks);
  } else if (w.relatie) {
    const [a, b] = w.relatie.map((n: string) => taak(p, n));
    const s: Gelezen['sequences'][number] =
      { id: generateId('seq'), predecessorId: a.id, successorId: b.id, type: w.type, lagDays: w.lagDagen ?? 0 };
    if (w.lagMinuten !== undefined) s.lagMinutes = w.lagMinuten;
    if (w.p6) s.p6StartAtPredecessorFinishBoundary = true;
    p.sequences.push(s);
  } else if (w.verplaats) {
    const t = taak(p, w.verplaats);
    const ouder = taak(p, w.onder);
    for (const x of p.tasks) x.childIds = x.childIds.filter(c => c !== t.id);
    t.parentId = ouder.id;
    ouder.childIds.push(t.id);
    // De volgorde van kinderen is de volgorde in de lijst: achteraan.
    p.tasks = [...p.tasks.filter(x => x.id !== t.id), t];
    applyWbsNumbering(p.tasks);
  } else if (w.nieuw) {
    const zoals = taak(p, w.zoals);
    const ouder = taak(p, w.onder);
    const t = structuredClone(zoals);
    t.id = generateId('task');
    t.name = w.nieuw;
    t.parentId = ouder.id;
    t.childIds = [];
    ouder.childIds.push(t.id);
    p.tasks.push(t);
    applyWbsNumbering(p.tasks);
  } else if (w.baseline) {
    // Zoals `saveBaseline` in de app (`baselineSlice.ts`).
    const r = reken(p);
    const id = generateId('baseline');
    p.baselines = [...(p.baselines ?? []), {
      id, name: w.baseline, createdAt: new Date().toISOString(),
      tasks: p.tasks.filter(isLeafTask).map(t => ({
        taskId: t.id, start: shownStart(t), finish: shownFinish(t), duration: t.time.scheduleDuration,
        isMilestone: t.isMilestone, ...(t.milestoneKind ? { milestoneKind: t.milestoneKind } : {}),
      })),
      projectEnd: r.projectEnd ?? '', projectDuration: r.projectDuration ?? 0,
    }];
    p.activeBaselineId = id;
    return;
  } else {
    throw new Error(`onbekende wijziging ${JSON.stringify(w)}`);
  }
  const r = reken(p);
  if (r.error) throw new Error(`doorrekenen mislukt: ${r.error}`);
}

function schrijf(p: Gelezen): string {
  // Zoals `buildWriteIFCInput` in de app, met de GlobalIds uit het ingelezen bestand.
  return writeIFC({
    project: p.project, calendar: p.calendar, tasks: p.tasks, sequences: p.sequences, resources: p.resources,
    assignments: p.assignments, activityCodeTypes: p.activityCodeTypes, customFieldDefs: p.customFieldDefs,
    customTaskTypes: p.customTaskTypes, resourceCalendars: p.resourceCalendars, baselines: p.baselines,
    activeBaselineId: p.activeBaselineId, comparedBaselineIds: p.comparedBaselineIds,
    ...(p.ifcGlobalIds ? { ifcGlobalIds: p.ifcGlobalIds } : {}),
  });
}

function tel(tekst: string, type: string): number {
  return (tekst.match(new RegExp(`^#\\d+\\s*=\\s*${type}\\s*\\(`, 'gm')) ?? []).length;
}

function lees(bestand: string) {
  const tekst = readFileSync(bestand, 'utf8');
  const p = readIFC(tekst);
  const fouten: string[] = [];
  if (p.tasks.length !== tel(tekst, 'IFCTASK')) fouten.push(`${p.tasks.length} taken gelezen, ${tel(tekst, 'IFCTASK')} in de tekst`);
  if (p.sequences.length !== tel(tekst, 'IFCRELSEQUENCE')) {
    fouten.push(`${p.sequences.length} relaties gelezen, ${tel(tekst, 'IFCRELSEQUENCE')} in de tekst`);
  }
  const ids = new Set(p.tasks.map(t => t.id));
  for (const t of p.tasks) if (t.parentId && !ids.has(t.parentId)) fouten.push(`'${t.name}' heeft een onbekende ouder`);
  const paren = p.sequences.map(s => `${s.predecessorId}>${s.successorId}`);
  if (new Set(paren).size !== paren.length) fouten.push('een relatie staat er dubbel');
  const naam = new Map(p.tasks.map(t => [t.id, t.name]));
  const r = reken(p);
  if (r.error) fouten.push(`doorrekenen: ${r.error}`);
  return {
    bestand,
    fouten,
    taken: p.tasks.map(t => ({
      naam: t.name, wbs: t.wbsCode, ouder: t.parentId ? naam.get(t.parentId) : null, duur: t.time.scheduleDuration,
    })),
    relaties: p.sequences.map(s => ({ van: naam.get(s.predecessorId), naar: naam.get(s.successorId), type: s.type })),
    baselines: (p.baselines ?? []).map(b => b.name),
  };
}

const [opdracht, ...rest] = process.argv.slice(2);
if (opdracht === 'lees') {
  let fout = false;
  for (const bestand of rest) {
    try {
      const r = lees(bestand);
      fout ||= r.fouten.length > 0;
      console.log(JSON.stringify(r));
    } catch (e) {
      fout = true;
      console.log(JSON.stringify({ bestand, fouten: [`lezen mislukt: ${(e as Error).message}`] }));
    }
  }
  process.exitCode = fout ? 1 : 0;
} else if (opdracht === 'wijzig') {
  const [uit, naar, json] = rest;
  const p = readIFC(readFileSync(uit, 'utf8'));
  wijzig(p, JSON.parse(json));
  writeFileSync(naar, schrijf(p));
} else {
  console.error('gebruik: lees <bestand.ifc>… | wijzig <in> <uit> <json>');
  process.exitCode = 2;
}
