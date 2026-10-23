// Exemplo para a captura do editor.
import { readFile } from "fs";

interface Point {
	x: number;
	y: number;
}

export class Counter {
	private count = 0;

	increment(step: number = 1): number {
		this.count += step;
		return this.count;
	}
}

function distance(a: Point, b: Point): number {
	const dx = a.x - b.x;
	const dy = a.y - b.y;
	if (dx === 0 && dy === 0) {
		return 0;
	}
	return Math.sqrt(dx * dx + dy * dy);
}

const items = [1, 2, 3].map((n) => ({ value: n * 2 }));
console.log(distance({ x: 0, y: 0 }, { x: 3, y: 4 }), items);
