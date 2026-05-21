#!/usr/bin/env node
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const upstreamDir = resolve(process.env.ENS_NORMALIZE_JS_DIR ?? '/tmp/ens-normalize.js');
const upstreamEntry = join(upstreamDir, 'dist', 'index.mjs');

if (!existsSync(upstreamEntry)) {
	console.error(`Could not find upstream ens-normalize.js at ${upstreamEntry}`);
	console.error('Set ENS_NORMALIZE_JS_DIR=/path/to/ens-normalize.js');
	process.exit(1);
}

const upstreamPackage = JSON.parse(readFileSync(join(upstreamDir, 'package.json'), 'utf8'));
const js = await import(pathToFileURL(upstreamEntry).href);

const names = [
	'raffy.eth',
	'RaFfY.eth',
	'vitalik.eth',
	'👩🏽‍⚕️.eth',
	'0️⃣1️⃣2️⃣3️⃣.eth',
	'àáâãāäåăąćĉčç.eth',
	'ガギグゲゴザジズゼゾ.eth',
	'nowzad.loopring.eth',
];

const cpsMixed = Array.from('RaFfY àáâãāäåăąćĉč 👩🏽‍⚕️', ch => ch.codePointAt(0));
let sink;

function nowNs() {
	return process.hrtime.bigint();
}

function elapsedNs(start) {
	return Number(nowNs() - start);
}

function runBatch(fn, iterations) {
	for (let i = 0; i < iterations; i++) {
		sink = fn();
	}
}

function calibrate(fn) {
	let iterations = 1;
	while (true) {
		const start = nowNs();
		runBatch(fn, iterations);
		const elapsed = elapsedNs(start);
		if (elapsed >= 25_000_000 || iterations >= 1 << 28) return iterations;
		iterations *= 2;
	}
}

function median(values) {
	const sorted = values.toSorted((a, b) => a - b);
	const mid = sorted.length >> 1;
	return sorted.length & 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function bench(fn, samples = 50) {
	runBatch(fn, 10_000);
	const iterations = calibrate(fn);
	const values = [];
	for (let i = 0; i < samples; i++) {
		const start = nowNs();
		runBatch(fn, iterations);
		values.push(elapsedNs(start) / iterations);
	}
	return {
		iterations,
		median: median(values),
		mean: values.reduce((a, b) => a + b, 0) / values.length,
		min: Math.min(...values),
		max: Math.max(...values),
	};
}

function readRustNs(group, parameter) {
	const file = parameter
		? join(root, 'target', 'criterion', group, parameter, 'new', 'estimates.json')
		: join(root, 'target', 'criterion', group, 'new', 'estimates.json');
	if (!existsSync(file)) return undefined;
	return JSON.parse(readFileSync(file, 'utf8')).mean.point_estimate;
}

function formatNs(ns) {
	if (ns === undefined || Number.isNaN(ns)) return '';
	if (ns >= 1000) return `${(ns / 1000).toFixed(2)} us`;
	return `${ns.toFixed(0)} ns`;
}

function row(label, rustNs, jsNs) {
	const ratio = rustNs ? jsNs / rustNs : undefined;
	return {
		benchmark: label,
		rust: formatNs(rustNs),
		js: formatNs(jsNs),
		'js/rust': ratio ? `${ratio.toFixed(2)}x` : '',
	};
}

console.log(`Node: ${process.version}`);
console.log(`JS upstream: ${upstreamPackage.name} ${upstreamPackage.version}`);
console.log(`JS entry: ${upstreamEntry}`);
console.log();

for (const name of names) {
	js.ens_normalize(name);
	js.ens_tokenize(name);
}
js.nfc(cpsMixed);

const rows = [];
for (const name of names) {
	const stat = bench(() => js.ens_normalize(name));
	rows.push(row(`ens_normalize/${name}`, readRustNs('ens_normalize', name), stat.median));
}
for (const name of names) {
	const stat = bench(() => js.ens_tokenize(name));
	rows.push(row(`ens_tokenize/${name}`, readRustNs('ens_tokenize', name), stat.median));
}
{
	const stat = bench(() => js.nfc(cpsMixed));
	rows.push(row('nfc/mixed', readRustNs('nfc_mixed'), stat.median));
}

console.table(rows);

if (typeof sink === 'undefined') {
	console.error('unreachable');
	process.exit(1);
}
