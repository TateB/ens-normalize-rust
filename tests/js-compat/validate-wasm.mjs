import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const require = createRequire(import.meta.url);
const wasm = require('./pkg/ens_normalize.js');
const here = dirname(fileURLToPath(import.meta.url));
const fixtureDir = join(here, '..', 'fixtures');

const TESTS = JSON.parse(readFileSync(join(fixtureDir, 'validate-tests.json'), 'utf8'));
const CUSTOM_TESTS = JSON.parse(readFileSync(join(fixtureDir, 'custom-tests.json'), 'utf8'));

function str_from_cps(cps) {
	const chunk = 4096;
	if (cps.length < chunk) return String.fromCodePoint(...cps);
	let buf = [];
	for (let i = 0; i < cps.length;) {
		buf.push(String.fromCodePoint(...cps.slice(i, i += chunk)));
	}
	return buf.join('');
}

function run_tests(fn, tests) {
	let errors = [];
	for (let test of tests) {
		let { name, norm, error } = test;
		if (typeof norm !== 'string') norm = name;
		try {
			let result = fn(name);
			if (error) {
				errors.push({ type: 'expected error', result, ...test });
			} else if (result !== norm) {
				errors.push({ type: 'wrong norm', result, ...test });
			}
		} catch (err) {
			if (!error) {
				errors.push({ type: 'unexpected error', result: err.message, ...test });
			}
		}
	}
	return errors;
}

function normalize_via_tokenize(name) {
	let cps = wasm.ens_tokenize(name).flatMap(token => {
		switch (token.type) {
			case 'disallowed': throw new Error('disallowed');
			case 'ignored': return [];
			case 'stop': return token.cp;
			default: return token.cps;
		}
	});
	let norm = str_from_cps(wasm.nfc(cps));
	if (wasm.ens_normalize(norm) !== norm) {
		throw new Error(`wrong: ${norm}`);
	}
	if (!norm) wasm.ens_normalize(name);
	return norm;
}

for (let [name, fn, tests] of [
	['ens_normalize validate', wasm.ens_normalize, TESTS],
	['ens_normalize custom', wasm.ens_normalize, CUSTOM_TESTS],
	['tokenize validate', normalize_via_tokenize, TESTS],
]) {
	let errors = run_tests(fn, tests);
	if (errors.length) {
		console.error(errors.slice(0, 10));
		throw new Error(`${name}: ${errors.length} errors`);
	}
	console.log(`PASS ${name} (${tests.length})`);
}
