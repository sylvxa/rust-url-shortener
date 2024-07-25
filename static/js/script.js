/*
* blahaj.mom
* url shortener
*/

const inputForm = document.querySelector("form")
const urlInput = document.querySelector(".url-input");
const expiryInput = document.querySelector(".expiry-date");

const generatedList = document.querySelector(".generated");

// UI STUFF
function setExpiryBounds() {
    let now = new Date();

    // Over-complicated code to get next month.
    let monthFromNow = new Date(now.getTime())

    let nextMonth = now.getMonth();
    if (nextMonth++ === 12) nextMonth = 0;

    monthFromNow.setMonth(nextMonth)

    // Get next year
    let yearFromNow = new Date(now.getTime());
    yearFromNow.setFullYear(yearFromNow.getFullYear() + 1);

    // Apply bounds
    expiryInput.min = now.toISOString().slice(0, -8);
    expiryInput.value = monthFromNow.toISOString().slice(0, -8);
    expiryInput.max = yearFromNow.toISOString().slice(0, -8);
}

setExpiryBounds();

function addLinkUiElement(id, destination) {
    const li = document.createElement("li");

    const src = document.createElement("a")
    const between = document.createElement("span")
    const dest = document.createElement("a")


    src.href = "/" + id;
    src.innerText = src.href;

    between.innerText = " -> "

    dest.href = destination;
    dest.innerText = destination;

    li.appendChild(src);
    li.appendChild(between);
    li.appendChild(dest);

    generatedList.prepend(li)
}

// THIS IS THE PART YOU WANT
inputForm.onsubmit = async function onsubmit(e) {
    e.preventDefault();

    if (!urlInput.checkValidity()) {
        console.log("Invalid url!")
        return false;
    }

    if (!expiryInput.checkValidity()) {
        console.log("Invalid expiry datetime!")
        return false;
    }

    e.target.setAttribute("disabled", true);

    const response = await fetch("/api/create", {
        method: "POST", // THIS PART \/
        body: JSON.stringify({ "destination": urlInput.value, "expires": expiryInput.valueAsNumber })
    });

    if (response.status === 201) { // WOOP WOOP! CREATED
        let route = await response.json();
        addLinkUiElement(route.id, route.destination)
    } else {
        alert(await response.text())
    }

    e.target.removeAttribute("disabled")
}

