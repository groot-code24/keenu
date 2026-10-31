// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// ESLint for the GNOME Shell extension: `npm ci && npx eslint`. Correctness rules only,
// after gnome-shell's own lint/eslintrc-gjs.yml; formatting is left to review.

import js from '@eslint/js';

// What GJS defines in every program.
const gjsGlobals = {
    ARGV: 'readonly',
    Debugger: 'readonly',
    GIRepositoryGType: 'readonly',
    imports: 'readonly',
    log: 'readonly',
    logError: 'readonly',
    print: 'readonly',
    printerr: 'readonly',
    console: 'readonly',
    setTimeout: 'readonly',
    clearTimeout: 'readonly',
    setInterval: 'readonly',
    clearInterval: 'readonly',
    TextEncoder: 'readonly',
    TextDecoder: 'readonly',
};

// What GNOME Shell adds in its own process (js/ui/environment.js).
const shellGlobals = {
    global: 'readonly',
    _: 'readonly',
    C_: 'readonly',
    N_: 'readonly',
    ngettext: 'readonly',
};

// A local `global`, `log` or `Proxy` hides the real one. Core no-shadow also flags the
// usual `const Page = GObject.registerClass(class Page …)`, so this checks globals only.
const noShadowGlobal = {
    meta: {
        type: 'problem',
        docs: {description: 'Disallow local names that hide a JavaScript, GJS or GNOME Shell global'},
        schema: [],
    },
    // Runs once per file: compares every local name with the globals above.
    create(context) {
        const {scopeManager} = context.sourceCode;
        return {
            // After the whole file is parsed, so every scope is known.
            'Program:exit'() {
                const globals = scopeManager.globalScope.set;
                for (const scope of scopeManager.scopes) {
                    if (scope.type === 'global')
                        continue;
                    for (const variable of scope.variables) {
                        const shadowed = globals.get(variable.name);
                        if (variable.name === '_' || !variable.defs.length || !shadowed || shadowed.defs.length)
                            continue;
                        context.report({
                            node: variable.identifiers[0],
                            message: `'${variable.name}' hides the global of that name.`,
                        });
                    }
                }
            },
        };
    },
};

export default [
    {ignores: ['target/', 'dist/']},
    {
        files: ['gnome-extension/**/*.js'],
        plugins: {isle: {rules: {'no-shadow-global': noShadowGlobal}}},
        languageOptions: {
            ecmaVersion: 'latest',
            sourceType: 'module',
            globals: {...gjsGlobals, ...shellGlobals},
        },
        linterOptions: {
            reportUnusedDisableDirectives: 'error',
        },
        rules: {
            ...js.configs.recommended.rules,
            'no-unused-vars': ['error', {
                args: 'after-used',
                argsIgnorePattern: '^_',
                varsIgnorePattern: '^_',
                caughtErrors: 'none',
            }],
            'no-empty': ['error', {allowEmptyCatch: true}],
            'isle/no-shadow-global': 'error',
            'no-redeclare': ['error', {builtinGlobals: true}],
            'no-self-compare': 'error',
            'no-template-curly-in-string': 'error',
            'no-unmodified-loop-condition': 'error',
            'no-unreachable-loop': 'error',
            'no-promise-executor-return': 'error',
            'no-unused-private-class-members': 'error',
            'no-useless-assignment': 'error',
            'no-implied-eval': 'error',
            'no-new-wrappers': 'error',
            'no-throw-literal': 'error',
            'prefer-promise-reject-errors': 'error',
            'array-callback-return': 'error',
            // GNOME 45 and newer load extensions as ES modules; the old `imports.*` is not allowed there.
            'no-restricted-properties': ['error',
                {object: 'imports', property: 'gi', message: "Import from 'gi://…' instead."},
                {object: 'imports', property: 'ui', message: "Import from 'resource:///org/gnome/shell/ui/…' instead."},
                {object: 'imports', property: 'misc', message: "Import from 'resource:///org/gnome/shell/misc/…' instead."},
                {object: 'imports', property: 'lang', message: 'Use ES classes and arrow functions.'},
                {object: 'Lang', property: 'bind', message: 'Use arrow functions or Function.prototype.bind.'},
            ],
        },
    },
];
