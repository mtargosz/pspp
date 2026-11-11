# pspp

Code a CLI command called `pspp` for `ps plus plus` or `ps++` to sum up all memory taken by processes grouped by their parent processes.
Where for output from `ps ax -o pid,ppid,rss,comm` the `ppid` is a self-referencing relationship, or a foreign key column that links back to its own primary key (the `pid` in this example).

- must be interactive and with mouse support 
	- but it should allow to perform all operations using keyboard primarily
- it should support simple return queries
- use colours so it is easy to see high usage (with bloody red for high / bad end and grass green for low / good end)
- use UI styles / techniques similar to [btop](https://github.com/aristocratos/btop)  
	- use the type of gentle shading style
	- use UTF-8 braille chars for graphs, charts, (progress) bars
	- use similar approach to main menu when pressing `esc` in `btop` with "options, help and quit" options
	- implement similar options to what is available in Main Menu > Options > General in `btop`
- support measuring various types of memory
	- using simple English, explain what they represent in help

