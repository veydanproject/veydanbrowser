<script lang="ts">
  import NoteRichEditor from '$lib/components/notes/NoteRichEditor.svelte';
  let content = $state('![shot.png](attachments/x/shot.png)\n');
  let ed: NoteRichEditor | null = $state(null);
</script>

<div style="display:flex; height: 80vh; gap: 8px; padding: 8px;">
  <div style="flex:1; display:flex; min-height:0; position:relative; border:1px solid #888;">
    <NoteRichEditor
      bind:this={ed}
      {content}
      noteId="x"
      baseDir="/tmp/notes"
      notes={[]}
      onchange={(md) => (content = md)}
      onwikilink={() => {}}
      onfiles={() => {}}
    />
  </div>
  <pre id="out" style="flex:1; margin:0; border:1px solid #888; font-size:12px;">{content}</pre>
</div>
