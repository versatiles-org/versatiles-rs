#!/usr/bin/env node

/**
 * Shared logging utilities for examples
 * Provides consistent, colorized output across all example files
 */

import chalk from 'chalk';

export const log = {
	// Title/header for the entire example
	title: (text) => {
		console.log(chalk.bold.cyan(`${text}\n`));
	},

	// Section header (e.g., "Example 1: Read a single tile")
	section: (text) => {
		console.log(chalk.bold.white(`\n${text}`));
	},

	// Success message with checkmark
	success: (text) => {
		console.log(chalk.green(`  ✓ ${text}`));
	},

	// Error message with cross
	error: (text) => {
		console.error(chalk.red(`  ✗ ${text}`));
	},

	// Warning message
	warning: (text) => {
		console.log(chalk.yellow(`  ⚠ ${text}`));
	},

	// Info/label with value
	info: (label, value) => {
		console.log(`  ${chalk.gray(label + ':')} ${value}`);
	},

	// Indented info (4-space)
	infoIndented: (label, value) => {
		console.log(`    ${chalk.gray(label + ':')} ${value}`);
	},

	// URL/endpoint (blue, clickable in most terminals)
	url: (label, url) => {
		console.log(`  ${chalk.gray(label + ':')} ${chalk.blue.underline(url)}`);
	},

	// File path (cyan)
	path: (label, filePath) => {
		console.log(`  ${chalk.gray(label + ':')} ${chalk.cyan(filePath)}`);
	},

	// Progress update (for convert-with-progress.mjs)
	progress: (data) => {
		const percentage = chalk.bold.yellow(`${data.percentage.toFixed(1)}%`);
		const tiles = chalk.gray(`(${data.position.toFixed(0)}/${data.total.toFixed(0)} tiles)`);
		const speed = chalk.cyan(`${data.speed.toFixed(0)} tiles/sec`);
		const eta = data.eta ? chalk.magenta(new Date(data.eta).toTimeString().split(' ')[0]) : chalk.gray('N/A');

		console.log(`  Progress: ${percentage} ${tiles} | ${speed} | ETA: ${eta}`);
	},

	// Plain text with consistent indentation
	text: (text, indent = 2) => {
		const spaces = ' '.repeat(indent);
		console.log(`${spaces}${text}`);
	},

	// Tile status (used in read-tiles.mjs)
	tileStatus: (coord, status, hasData) => {
		const coordStr = chalk.gray(`${coord.z}/${coord.x}/${coord.y}`);
		const statusStr = hasData ? chalk.green(`✓ ${status}`) : chalk.red(`✗ ${status}`);
		console.log(`  Tile ${coordStr}: ${statusStr}`);
	},
};
