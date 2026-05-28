<script lang="ts">
  let now = $state(new Date());
  $effect(() => {
    const id = setInterval(() => { now = new Date(); }, 1000);
    return () => clearInterval(id);
  });
  let time = $derived(now.toLocaleTimeString());
  let date = $derived(now.toLocaleDateString());
</script>

<div class="clock">
  <div class="time">{time}</div>
  <div class="date">{date}</div>
</div>
