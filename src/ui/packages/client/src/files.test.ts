import { describe, it, expect } from 'vitest';
import type {
  FileEntry,
  FileOperation,
  FilesEvent,
  Places,
  PreviewPayload,
} from './files';

/** Narrowing helper that proves `FilesEvent` discriminates on `event`. */
function describeEvent(event: FilesEvent): string {
  switch (event.event) {
    case 'changed':
      return event.path;
    case 'size':
      return `${event.path}:${event.bytes}:${event.complete}`;
    case 'operation_complete':
      return event.operation.kind;
    case 'operation_failed':
      return event.message;
  }
}

/** Narrowing helper that proves `FileOperation` discriminates on `kind`. */
function operationDestination(operation: FileOperation): string {
  switch (operation.kind) {
    case 'copy':
    case 'move':
    case 'compress':
      return operation.destination;
    case 'rename':
      return operation.new_name;
    case 'duplicate':
    case 'extract':
      return operation.path;
    case 'new_folder':
    case 'new_file':
      return `${operation.parent}/${operation.name}`;
    case 'trash':
    case 'delete':
      return operation.paths.join(',');
  }
}

describe('FileEntry', () => {
  it('assigns a file payload with recursive_size null and parses round-trip', () => {
    const entry: FileEntry = {
      name: 'notes.txt',
      path: '/home/user/notes.txt',
      kind: 'file',
      size: 2048,
      recursive_size: null,
      modified_epoch_seconds: 1781663172,
      owner: 'user',
      permissions: 'rw-r--r--',
      permission_class: 'normal',
      symlink_target: null,
      content_kind: 'document',
    };

    const roundTrip = JSON.parse(JSON.stringify(entry)) as FileEntry;
    expect(roundTrip).toEqual(entry);
    expect(roundTrip.recursive_size).toBeNull();
    expect(roundTrip.symlink_target).toBeNull();
  });

  it('assigns a symlink payload carrying symlink_target', () => {
    const raw =
      '{"name":"latest","path":"/var/log/latest","kind":"symlink","size":0,' +
      '"recursive_size":null,"modified_epoch_seconds":1781663172,"owner":"root",' +
      '"permissions":"rwxrwxrwx","permission_class":"root_owned",' +
      '"symlink_target":"/var/log/app-2026.log","content_kind":"other"}';

    const entry: FileEntry = JSON.parse(raw);
    expect(entry.kind).toBe('symlink');
    expect(entry.symlink_target).toBe('/var/log/app-2026.log');
    expect(entry.permission_class).toBe('root_owned');
  });

  it('assigns a directory payload with a recursive_size number', () => {
    const entry: FileEntry = {
      name: 'projects',
      path: '/home/user/projects',
      kind: 'directory',
      size: 4096,
      recursive_size: 1048576,
      modified_epoch_seconds: 1781663172,
      owner: 'user',
      permissions: 'rwxr-xr-x',
      permission_class: 'normal',
      symlink_target: null,
      content_kind: 'other',
    };

    expect(entry.recursive_size).toBe(1048576);
  });
});

describe('FileOperation discriminated union', () => {
  it('assigns a move operation with sources and destination', () => {
    const operation: FileOperation = {
      kind: 'move',
      sources: ['/home/user/a.txt', '/home/user/b.txt'],
      destination: '/home/user/archive',
    };

    expect(operation.kind).toBe('move');
    expect(operation.sources).toEqual(['/home/user/a.txt', '/home/user/b.txt']);
    expect(operation.destination).toBe('/home/user/archive');
    expect(operationDestination(operation)).toBe('/home/user/archive');
  });

  it('narrows a rename operation to new_name', () => {
    const operation: FileOperation = {
      kind: 'rename',
      path: '/home/user/old.txt',
      new_name: 'new.txt',
    };

    expect(operationDestination(operation)).toBe('new.txt');
  });

  it('narrows a new_folder operation to parent and name', () => {
    const operation: FileOperation = {
      kind: 'new_folder',
      parent: '/home/user',
      name: 'fresh',
    };

    expect(operationDestination(operation)).toBe('/home/user/fresh');
  });
});

describe('FilesEvent discriminated union', () => {
  it('parses a changed event round-trip', () => {
    const raw = '{"event":"changed","path":"/home/user/notes.txt"}';
    const event: FilesEvent = JSON.parse(raw);
    expect(event.event).toBe('changed');
    expect(describeEvent(event)).toBe('/home/user/notes.txt');
  });

  it('parses a size event with bytes and complete', () => {
    const raw = '{"event":"size","path":"/home/user/projects","bytes":1048576,"complete":false}';
    const event: FilesEvent = JSON.parse(raw);
    expect(event.event).toBe('size');
    expect(describeEvent(event)).toBe('/home/user/projects:1048576:false');
  });

  it('parses an operation_complete event carrying a FileOperation', () => {
    const raw =
      '{"event":"operation_complete","operation":{"kind":"trash","paths":["/home/user/tmp"]}}';
    const event: FilesEvent = JSON.parse(raw);
    expect(event.event).toBe('operation_complete');
    expect(describeEvent(event)).toBe('trash');
  });

  it('parses an operation_failed event with a message', () => {
    const raw = '{"event":"operation_failed","message":"permission denied"}';
    const event: FilesEvent = JSON.parse(raw);
    expect(event.event).toBe('operation_failed');
    expect(describeEvent(event)).toBe('permission denied');
  });
});

describe('Places and PreviewPayload', () => {
  it('assigns a Places snapshot with pins and drives', () => {
    const places: Places = {
      pins: [{ label: 'Home', path: '/home/user' }],
      drives: [
        { label: 'System', mount_point: '/', total_bytes: 500000, free_bytes: 120000 },
      ],
    };

    const roundTrip = JSON.parse(JSON.stringify(places)) as Places;
    expect(roundTrip).toEqual(places);
  });

  it('assigns a PreviewPayload', () => {
    const preview: PreviewPayload = { kind: 'text', data: 'hello world' };
    expect(preview.kind).toBe('text');
    expect(preview.data).toBe('hello world');
  });
});
