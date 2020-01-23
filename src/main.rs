extern crate clap;
extern crate num_cpus;
extern crate shellexpand;
extern crate think_thank_rust;

use std::{fs, thread};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::{Arc, mpsc, Mutex};
use std::sync::mpsc::Sender;

use clap::{App, Arg};

fn main() {
    let mut num_cpus = num_cpus::get();
    let num_cpus_str = num_cpus.to_string();
    let app = App::new("Find string in file")
        .about("Finds a string in the files of a directory. Prints all the paths to the files containing the string.")
        .version("v0.1.0")
        .author("Nils")
        .arg(Arg::with_name("recursive")
            .short("r")
            .long("recursive")
            .help("Recursively look in sub directoriess"))
        .arg(Arg::with_name("match-case")
            .short("c")
            .long("match-case")
            .help("Whether the string in the file should match the case of the query"))
        .arg(Arg::with_name("jobs")
            .short("j")
            .long("jobs")
            .help("number of jobs to run in parallel, defaults to number of available cpu threads")
            .default_value(&num_cpus_str)
            .takes_value(true))
        .arg(Arg::with_name("query")
            .required(true)
            .help("The string to look for"))
        .arg(Arg::with_name("directory")
            .default_value("./")
            .help("The directory where the search should start"));

    let matches = app.get_matches();

    let search_dir = shellexpand::tilde(matches.value_of("directory").unwrap());
    let search_dir = Path::new(search_dir.deref());
    if !search_dir.is_dir() {
        println!("Error: {:?}, is not a directory", search_dir);
        return;
    }
    let thread_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(1));
    let collected = Arc::new(Mutex::new(Vec::new()));
    let match_case = matches.is_present("match-case");
    static mut QUERY: String = String::new();
    unsafe { QUERY = matches.value_of("query").unwrap().to_string(); }
    if !match_case {
        unsafe { QUERY = QUERY.to_ascii_lowercase(); }
    }
    if let Ok(num) = matches.value_of("jobs").unwrap().parse() {
        num_cpus = num;
    } else {
        println!("Error expected positive integer as argument, got: {:?}", matches.value_of("jobs").unwrap());
        return;
    }
    let (sender, receiver) = mpsc::channel();
    unsafe {
        search(search_dir,
               &QUERY,
               match_case,
               matches.is_present("recursive"),
               collected.clone(),
               thread_count.clone(),
               num_cpus,
               sender,
               0);
    }
    while *thread_count.lock().unwrap() > 1 {
        receiver.recv().unwrap();
    }
    println!("Results:");
    let collected = collected.lock().unwrap();
    for result in &*collected {
        println!("{}", result);
    }
}

fn search(
    path: &Path,
    query: &'static str,
    match_case: bool,
    recursive: bool,
    collected: Arc<Mutex<Vec<String>>>,
    thread_count: Arc<Mutex<usize>>,
    max_threads: usize,
    thread_stop_notifier: Sender<()>,
    depth: usize,
) {
    if path.is_file() {
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            if reader.lines().any(|line| {
                if let Ok(mut line) = line {
                    if !match_case {
                        line = line.to_ascii_lowercase();
                    }
                    if line.contains(&query) {
                        return true;
                    }
                }
                false
            }) {
                let mut c = collected.lock().unwrap();
                c.deref_mut().push(path.to_str().unwrap().to_string())
            }
        }
    } else if (recursive || depth == 0) & &path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let new_path = entry.path();
                    let col_clone = collected.clone();
                    let tc_clone = thread_count.clone();
                    let tsn = thread_stop_notifier.clone();
                    if *thread_count.lock().unwrap() < max_threads {
                        *thread_count.lock().unwrap() += 1;
                        thread::spawn(move || {
                            search(&new_path, query, match_case, recursive, col_clone, tc_clone.clone(), max_threads, tsn.clone(), depth + 1);
                            *tc_clone.lock().unwrap() -= 1;
                            tsn.send(()).unwrap();
                        });
                    } else {
                        search(&new_path, query, match_case, recursive, col_clone, tc_clone, max_threads, tsn, depth + 1);
                    }
                }
            }
        }
    }
}
