// JavaScript Fixture with classes, functions, exports, imports, require, arrow functions
const { helperUtil } = require('./helper.js');
import { extraUtil } from './extra.mjs';

export class Calculator {
  add(a, b) {
    helperUtil();
    return a + b;
  }
}

export function computeTotal(x, y) {
  const calc = new Calculator();
  extraUtil();
  return calc.add(x, y);
}

export const multiply = (a, b) => {
  return a * b;
};
