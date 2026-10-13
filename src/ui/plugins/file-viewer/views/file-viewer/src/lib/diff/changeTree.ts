// Folder tree over the review entries of one sidebar section, for the qv diff
// Changes sidebar's tree view (docs/plans/2026-10-13-qv-diff-tree-view-design.md).
// Pure and Svelte-free, in the task-manager tree.ts style: build the tree,
// compact chains of folders that hold no files and a single child folder,
// order folders and files together alphabetically, and flatten into rows with
// a depth. `compareTreePaths` is that same order on bare paths, so the main
// pane can be sorted to match the sidebar without building a tree.
import type { ReviewEntry } from './reviewModel';

export type SidebarView = 'tree' | 'list';

export interface FolderNode {
	/** Display name: one path segment, or several joined by '/' after compaction. */
	name: string;
	/** Full path of the deepest folder this node stands for; '' for the root. Folding keys on it. */
	path: string;
	/** The first segment of `name`, used to order this folder among its siblings. */
	sortName: string;
	folders: FolderNode[];
	files: ReviewEntry[];
}

export type TreeRow =
	| { kind: 'folder'; depth: number; folder: FolderNode; expanded: boolean }
	| { kind: 'file'; depth: number; entry: ReviewEntry };

interface MutableFolder {
	name: string;
	path: string;
	folders: Map<string, MutableFolder>;
	files: ReviewEntry[];
}

type TreeChild = { kind: 'folder'; folder: FolderNode; sortName: string } | { kind: 'file'; entry: ReviewEntry; sortName: string };

function baseName(path: string): string {
	return path.slice(path.lastIndexOf('/') + 1);
}

/**
 * Orders two names: locale order, then code-unit order when the locale calls them equal but they differ
 * (for example NFC and NFD spellings of the same accented letter). Returns 0 only for identical strings.
 */
function compareNames(left: string, right: string): number {
	const order = left.localeCompare(right);
	if (order !== 0) {
		return order;
	}
	if (left === right) {
		return 0;
	}
	return left < right ? -1 : 1;
}

/** Orders two paths the way the tree reads them: segment by segment, folders and files mixed. */
export function compareTreePaths(left: string, right: string): number {
	const leftSegments = left.split('/');
	const rightSegments = right.split('/');
	const sharedLength = Math.min(leftSegments.length, rightSegments.length);
	for (let index = 0; index < sharedLength; index++) {
		const order = compareNames(leftSegments[index], rightSegments[index]);
		if (order !== 0) {
			return order;
		}
	}
	return leftSegments.length - rightSegments.length;
}

function sectionRank(entry: ReviewEntry): number {
	return entry.section === 'staged' ? 1 : 0;
}

/** Unstaged before Staged (as `reviewEntries` does), then tree order within each section. */
export function sortEntriesForTree(entries: readonly ReviewEntry[]): ReviewEntry[] {
	return [...entries].sort((left, right) => sectionRank(left) - sectionRank(right) || compareTreePaths(left.path, right.path));
}

function compactFolder(folder: MutableFolder): FolderNode {
	let current = folder;
	let name = folder.name;
	while (current.files.length === 0 && current.folders.size === 1) {
		const [onlyChild] = current.folders.values();
		name = `${name}/${onlyChild.name}`;
		current = onlyChild;
	}
	return toFolderNode(current, name, folder.name);
}

function toFolderNode(folder: MutableFolder, name: string, sortName: string): FolderNode {
	return {
		name,
		path: folder.path,
		sortName,
		folders: [...folder.folders.values()].map(compactFolder),
		files: [...folder.files],
	};
}

/**
 * Groups one section's entries into a folder tree whose single-child folder chains are compacted.
 * Expects repository-relative paths with non-empty segments (no leading or trailing slash).
 */
export function buildChangeTree(entries: readonly ReviewEntry[]): FolderNode {
	const root: MutableFolder = { name: '', path: '', folders: new Map(), files: [] };
	for (const entry of entries) {
		const segments = entry.path.split('/');
		let folder = root;
		for (const segment of segments.slice(0, -1)) {
			let child = folder.folders.get(segment);
			if (child === undefined) {
				child = { name: segment, path: folder.path === '' ? segment : `${folder.path}/${segment}`, folders: new Map(), files: [] };
				folder.folders.set(segment, child);
			}
			folder = child;
		}
		folder.files.push(entry);
	}
	return toFolderNode(root, '', '');
}

/** Orders siblings by name, then a file before a folder of the same name (matching `compareTreePaths`). */
function compareChildren(left: TreeChild, right: TreeChild): number {
	const order = compareNames(left.sortName, right.sortName);
	if (order !== 0) {
		return order;
	}
	if (left.kind === right.kind) {
		return 0;
	}
	return left.kind === 'file' ? -1 : 1;
}

function orderedChildren(folder: FolderNode): TreeChild[] {
	const children: TreeChild[] = [
		...folder.folders.map((child): TreeChild => ({ kind: 'folder', folder: child, sortName: child.sortName })),
		...folder.files.map((entry): TreeChild => ({ kind: 'file', entry, sortName: baseName(entry.path) })),
	];
	return children.sort(compareChildren);
}

/** Flattens a tree into display rows; a folder whose path is in `collapsedPaths` hides its descendants. */
export function flattenChangeTree(root: FolderNode, collapsedPaths: ReadonlySet<string>): TreeRow[] {
	const rows: TreeRow[] = [];
	function visit(folder: FolderNode, depth: number): void {
		for (const child of orderedChildren(folder)) {
			if (child.kind === 'folder') {
				const expanded = !collapsedPaths.has(child.folder.path);
				rows.push({ kind: 'folder', depth, folder: child.folder, expanded });
				if (expanded) {
					visit(child.folder, depth + 1);
				}
			} else {
				rows.push({ kind: 'file', depth, entry: child.entry });
			}
		}
	}
	visit(root, 0);
	return rows;
}

/** Every entry below `folder`, in tree order. */
export function entriesUnder(folder: FolderNode): ReviewEntry[] {
	const result: ReviewEntry[] = [];
	function visit(current: FolderNode): void {
		for (const child of orderedChildren(current)) {
			if (child.kind === 'folder') {
				visit(child.folder);
			} else {
				result.push(child.entry);
			}
		}
	}
	visit(folder);
	return result;
}
