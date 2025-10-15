# nhentai-favs
A local backup tool for nhentai favorites list.
This project is tailored to my personal needs and may not be suitable for you.

## What & Why
I needed to export my g^^ning material info in case nhentai purges some of it or the site goes down someday.
I'm not a programmer, so I used AI and some documentation to create this for my personal use only.
**I don't recommend using it**, but if you do, read the "How to Use" section.

## How to Use
Before explaining how to use this, let me give a quick overview:
This is inspired by [nhentai-favorites](https://github.com/phillychi3/nhentai-favorites) but it lacked some info I needed, so I made this project.
In exchange for that extra info, the initial data-grabbing process is very slow and much more network-intensive. Here's why:

**nhentai-favorites** works at the page level. If you have 25 favorite books, it sends only 1 HTTP request. That's it—you get the English title and tags for 25 manga with just 1 request.
But I needed the Japanese title too. For that, I make individual gallery-level network requests, so that's 25 requests (+1 for the page). Here's a rough comparison:

* nhentai-favorites: 1000 favs -> 40 network requests
* nhentai-favs:     1000 favs -> (40 + 1000) requests + another 1000 if the thumbnail download option is **enabled**

-----
Tip: Use `--help` for all the available commands

Fetch all favorite list (not recommended if you have alot books):
```
./nhentai-favs --outpath /path/to/output/folder
```
Fetch certain page range:
```
./nhentai-favs --page-range 1-10 --outpath /path/to/output/folder
```
For only one certain page (e.g. page 2):
```
./nhentai-favs --page-range 2-2 --outpath /path/to/output/folder
```
Download thumbnail image alongside:
```
./nhentai-favs --thumbnail --page-range 1-10 --outpath /path/to/output/folder
```
Convert a existing database to CSV file:
```
./nhentai-favs --cvt-csv --outpath /path/to/output/folder
```

### Important Info
* `--outpath` is mendetory, this is where the actual database file maintained, also you keep your `config.json` file in this folder.
* You have to must edit `config.json` file to include your browser's user agent and nhentai site's cookies.

**VERY IMPORTANT** : If your favourite list is big, then consider using `--page-range` option with 10 to 15 pages at a time, 
then try other 10-15 pages some other time until you cover all your fav pages.
But remember, don't add new favourite books (in the website) until you're finished backing up all pages (i mean you definitely can, if you know what you're doing). Once all pages are done, 
then moving onwards whenever you need to update the database just run the typical command without range option:
```
./nhentai-favs --outpath /path/to/output/folder
```
Anyways, make sure you never delete database (`nfavs.db`) file, you can delete the csv file exported from database, because you can always export that if database file exists.
And db file is very essential for tracking of your data.

### How to get cookies
Grab your browser's user agent from this [website](https://www.whatismybrowser.com/detect/what-is-my-user-agent/).

In Desktop you can grab your nhentai site cookies from Developer Tools option.
Alternative; use this [browser extension](https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc). It's [open source](https://github.com/kairi003/Get-cookies.txt-LOCALLY).

In Android use Firefox browser and this [browser extension](https://addons.mozilla.org/en-US/android/addon/cookies-txt/). It's [open source](https://github.com/hrdl-github/cookies-txt) too. And you can execute the binary in termux.
