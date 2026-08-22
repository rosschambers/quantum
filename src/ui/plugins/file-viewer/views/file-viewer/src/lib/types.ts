export type ViewerFileType = 'markdown' | 'json' | 'code' | 'text' | 'image' | 'video';

export interface ViewerFileInfo {
    content: string;
    file_type: ViewerFileType;
    language?: string;
    filename: string;
    directory: string;
    mime_type?: string;
    size: number;
    uri?: string;
}
