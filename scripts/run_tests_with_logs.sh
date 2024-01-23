#!/bin/bash

yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts
