// Combines two independent `file-viewer.read` results into the single-file,
// non-stageable `ChangeSet` the `qv <a> <b>` read-only compare source feeds
// through the SAME `DiffView` pipeline as a git change set (design doc,
// "qv <a> <b>" row). There is no git identity in a pair comparison — no
// repository, no blob ids — so `repository_root` is left empty (DiffView's
// header derives its title from `base_label`/`target_label` instead, never
// `repository_root`, for a pair source) and each side's `blob` is a
// placeholder (the file's own path), used only to satisfy `FileSide`'s
// shape. Nothing in the review pipeline compares pair-mode blob ids:
// `reviewEntries` only compares blobs for STAGEABLE sets, and this change
// set is always `stageable: false`.
import type { ChangeSet, ChangedFile, FileSide } from '@quantum/client';

export interface PairFile {
	/** The path exactly as passed to `file-viewer.read` (already `realpath`ed by the `qv` wrapper). */
	path: string;
	content: string;
	language?: string;
	/** True for file types that are not line-diffable text (image, video). */
	binary: boolean;
}

function side(file: PairFile): FileSide {
	return {
		content: file.binary ? undefined : file.content,
		blob: file.path,
		mode: '100644',
		binary: file.binary,
		too_large: false,
	};
}

/** Builds the `ChangeSet` `DiffView` renders for a `qv <a> <b>` comparison. */
export function pairChangeSet(left: PairFile, right: PairFile): ChangeSet {
	const file: ChangedFile = {
		path: right.path,
		language: right.language ?? left.language,
		base: side(left),
		target: side(right),
		untracked: false,
	};
	return {
		repository_root: '',
		base_label: left.path,
		target_label: right.path,
		stageable: false,
		files: [file],
	};
}
