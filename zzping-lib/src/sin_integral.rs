#![allow(clippy::excessive_precision, non_upper_case_globals)]
// from http://www.mymathlib.com/functions/sin_cos_integrals.html
// All this code... because I want the integral of sinc(x) from -inf to x. hah.

const PI2: f64 = std::f64::consts::FRAC_PI_2;
const EULER_GAMMA: f64 = 0.577215664901532860606512090;
const AUX_ASYMP_CUTOFF: f64 = 48.0;

// from math.h
fn fabsl(x: f64) -> f64 {
    x.abs()
}

fn logl(x: f64) -> f64 {
    x.ln()
}

fn expl(x: f64) -> f64 {
    // e^x
    f64::exp(x)
}

fn sinl(x: f64) -> f64 {
    f64::sin(x)
}

fn cosl(x: f64) -> f64 {
    f64::cos(x)
}
const LDBL_MAX: f64 = f64::MAX;

const a_x_ge_1_le_4_fi: [f64; 17] = [
    3.131622691136541251894e+6,
    5.865887504115410010938e+8,
    1.634852375578508416146e+10,
    1.592481384106901732624e+11,
    7.184770514348595264787e+11,
    1.726730020205455640781e+12,
    2.397017133822436251930e+12,
    2.020697105077248035167e+12,
    1.067232555649863576986e+12,
    3.595836616885923865165e+11,
    7.789746108788072914678e+10,
    1.083563302486680874140e+10,
    9.574882063563057212637e+8,
    5.257964657853357906628e+7,
    1.727886704287183044067e+6,
    3.186889399585378551937e+4,
    2.926771594419498165548e+2,
];

const b_x_ge_1_le_4_fi: [f64; 17] = [
    4.436542812456388065099e+7,
    3.071881739597743437918e+9,
    5.510695064187223810111e+10,
    4.064528338807937104680e+11,
    1.502383531521515631047e+12,
    3.100277228892702060035e+12,
    3.813360580503500561372e+12,
    2.914078460895404297552e+12,
    1.419825394894026616675e+12,
    4.475808471509418423489e+11,
    9.178793064550390753770e+10,
    1.220855825313365329368e+10,
    1.040609666863148007442e+9,
    5.554659381265650120714e+7,
    1.786400496083247945938e+6,
    3.243425514601407346662e+4,
    2.946771482142805033246e+2,
];

const a_x_ge_1_le_4_gi: [f64; 20] = [
    9.011634207324336137169e+5,
    7.479818286024998460948e+8,
    4.151156375407831323555e+10,
    7.527803170191763096250e+11,
    6.273399733237371076085e+12,
    2.814715541899249302011e+13,
    7.442080767131902041599e+13,
    1.227172725914716222093e+14,
    1.309767511841246149009e+14,
    9.271225348708999857908e+13,
    4.418956912530285701879e+13,
    1.429482655697021907140e+13,
    3.143569573598121793475e+12,
    4.678982847861465840256e+11,
    4.663335634051774987907e+10,
    3.055838078958224702739e+9,
    1.280057381534594891504e+8,
    3.283789466836908440869e+6,
    4.818060733773778820102e+4,
    3.575079810165216346615e+2,
];

const b_x_ge_1_le_4_gi: [f64; 20] = [
    3.473778902563924058876e+8,
    2.845671273312673204906e+10,
    6.887224173494194811858e+11,
    7.375036329278632360411e+12,
    4.176080452260044111884e+13,
    1.381922611468670308990e+14,
    2.845010797960102251342e+14,
    3.797756529707299562974e+14,
    3.379834764627141276920e+14,
    2.042485720392467096358e+14,
    8.475284361332246080070e+13,
    2.427267535696371015657e+13,
    4.796526275835169465639e+12,
    6.502922666518397649596e+11,
    5.978699373743563855764e+10,
    3.657882344026889055127e+9,
    1.447319540468370039281e+8,
    3.546640511226990055118e+6,
    5.024169863961865278657e+4,
    3.635079182389876878272e+2,
];

const a_x_ge_4_le_12_fi: [f64; 13] = [
    8.629036659345232923178e+15,
    9.470743102805298529462e+16,
    1.568021122342358329530e+17,
    9.015832733196613551192e+16,
    2.373367953145819143578e+16,
    3.275410521405716571530e+15,
    2.556227076494300926751e+14,
    1.177702886070105437976e+13,
    3.270951405687038350516e+11,
    5.490274976211303931784e+9,
    5.472393083052247561960e+7,
    3.092021722264748314966e+5,
    8.929706311321410431845e+2,
];

const b_x_ge_4_le_12_fi: [f64; 13] = [
    3.688863305339062824609e+16,
    1.876827085370834659310e+17,
    2.346441340788672968041e+17,
    1.163521165422882838284e+17,
    2.802875478319020095488e+16,
    3.655276206330722751898e+15,
    2.748086189268929173963e+14,
    1.234713082649844595139e+13,
    3.371469088301994839064e+11,
    5.594066018240082151795e+9,
    5.532510775982731500507e+7,
    3.109681134906426986992e+5,
    8.949706311313908973230e+2,
];
const a_x_ge_4_le_12_gi: [f64; 15] = [
    9.760124389962086158256e+17,
    2.768135717060729724771e+19,
    7.269925460678163397319e+19,
    6.335403079477117544205e+19,
    2.521611356160483301958e+19,
    5.326725622049037767865e+18,
    6.500059887901948040470e+17,
    4.822924381737713175777e+16,
    2.243854020350856804468e+15,
    6.651665288514689504327e+13,
    1.260766706261790080221e+12,
    1.513530299892650289088e+10,
    1.121138426325906850959e+8,
    4.860629732996342070790e+5,
    1.106668096706748177652e+3,
];
const b_x_ge_4_le_12_gi: [f64; 15] = [
    2.816012818637797223215e+19,
    1.453987140070137119268e+20,
    2.126102863700101349915e+20,
    1.330695747874759471888e+20,
    4.272711621417996420464e+19,
    7.778583943891982632436e+18,
    8.532286887837346444555e+17,
    5.856829003446583158094e+16,
    2.573169752130006183553e+15,
    7.312517856321160958464e+13,
    1.343719194435306279692e+12,
    1.577108016388663217206e+10,
    1.149410763356329587152e+8,
    4.926189818912136539829e+5,
    1.112668096702659763721e+3,
];
const a_x_ge_12_le_48_fi: [f64; 10] = [
    8.190718946165709238422e+17,
    1.209912798380869069939e+18,
    2.685711451753038556686e+17,
    2.031432644806673394287e+16,
    6.849516346373244528380e+14,
    1.167908359237227948685e+13,
    1.071365422608890062545e+11,
    5.395836264116777645374e+8,
    1.462073394608352079917e+6,
    1.959326763594685895502e+3,
];
const b_x_ge_12_le_48_fi: [f64; 10] = [
    1.759376483182613052616e+18,
    1.549737809630230245083e+18,
    3.002314821022841548975e+17,
    2.150253471166368305136e+16,
    7.064600781175281798566e+14,
    1.188341971751225609460e+13,
    1.081876692043348699994e+11,
    5.424692186656225562683e+8,
    1.465972048135541454369e+6,
    1.961326763594685895323e+3,
];
const a_x_ge_12_le_48_gi: [f64; 11] = [
    5.524091612614961621464e+19,
    1.284075904576105184520e+20,
    3.447334407523257944528e+19,
    3.121715037272484722094e+18,
    1.282539019600256176592e+17,
    2.740263968387649522824e+15,
    3.267265290103262920765e+13,
    2.245923126260050126684e+11,
    8.923806059854096302378e+8,
    1.985082566703293127903e+6,
    2.254025115381787893881e+3,
];
const b_x_ge_12_le_48_gi: [f64; 11] = [
    3.247999301164088453284e+20,
    2.442688918303073183435e+20,
    4.767807497134760332700e+19,
    3.740845893032137972381e+18,
    1.425986072860589430641e+17,
    2.920317933370183472849e+15,
    3.395218149102856121458e+13,
    2.297881510221565965240e+11,
    9.041055792759368518992e+8,
    1.998522717395583928785e+6,
    2.260025115381787888363e+3,
];
const n_x_ge_1_le_4_fi: usize = a_x_ge_1_le_4_fi.len();
const n_x_ge_1_le_4_gi: usize = a_x_ge_1_le_4_gi.len();
const n_x_ge_4_le_12_fi: usize = a_x_ge_4_le_12_fi.len();
const n_x_ge_4_le_12_gi: usize = a_x_ge_4_le_12_gi.len();
const n_x_ge_12_le_48_fi: usize = a_x_ge_12_le_48_fi.len();
const n_x_ge_12_le_48_gi: usize = a_x_ge_12_le_48_gi.len();

// The cos integral, Ci(x), is the integral with integrand `- cos(t) / t`
// where the integral extends from x to inf.
fn xcos_integral_ci(x: f64) -> f64 {
    if x == 0.0 {
        return -LDBL_MAX;
    }
    logl(fabsl(x)) + EULER_GAMMA - xpower_series_cin(x)
}

//  Description:                                                              //
//     This function returns a rational polynomial approximation to the       //
//     auxiliary sin integral for the argument x.  The rational polynomial    //
//     has the form p(x) / x q(x) where p(x) and q(x) are monic polynomials   //
//     of degree 2n,                                                          //
//                p(x) = x^2n + a[n-1] x^2(n-1) + ... + a[0]                  //
//          and   q(x) = x^2n + b[b-1] x^2(n-1) + ... + b[0].                 //
//                                                                            //
//  Arguments:                                                                //
//     f64  x  The argument of the auxiliary sin integral fi().       //
//     f64 a[] The arrays of coefficients of the numerator.           //
//     f64 b[] The arrays of coefficients of the denominator.         //
//     int          n  The half-order of the two polynomials.                 //
fn fi_rational_polynomial(x: f64, a: &[f64], b: &[f64], n: usize) -> f64 {
    let xx: f64 = x * x;
    let mut numerator: f64 = xx + a[n - 1];
    let mut denominator: f64 = xx + b[n - 1];
    let mut i = n as i64 - 2;
    // FIXME: - add a for in range
    while i >= 0 {
        numerator *= xx;
        numerator += a[i as usize];
        denominator *= xx;
        denominator += b[i as usize];
        i -= 1;
    }
    (numerator / denominator) / x
}

//     For a large argument x, the auxiliary sin integral, fi(x), can be      //
//     expressed as the asymptotic series                                     //
//      fi(x) ~ 1/x - 2/x^3 + 24/x^5 - 720/x^7 + ... + (2j)!/x(-x^2)^j + ...  //
fn asymptotic_series_fi(x: f64) -> f64 {
    let mut term: f64 = 1.0;
    let xx: f64 = -x * x;
    let mut xn: f64 = 1.0;
    let mut factorial: f64 = 1.0;
    let mut fi: f64 = 0.0;
    let mut old_term: f64 = 0.0;
    let mut j = 2;

    loop {
        fi += term;
        factorial *= (j * (j - 1)) as f64;
        xn *= xx;
        term = factorial / xn;
        j += 2;
        if fabsl(term) >= fabsl(old_term) {
            break;
        }
        old_term = term;
    }

    fi / x
}

//  Description:                                                              //
//     This function returns a rational polynomial approximation to the       //
//     auxiliary cos integral for the argument x.  The rational polynomial    //
//     has the form p(x) / x^2 q(x) where p(x) and q(x) are monic polynomials //
//     of degree 2n,                                                          //
//                p(x) = x^2n + a[n-1] x^2(n-1) + ... + a[0]                  //
//          and   q(x) = x^2n + b[n-1] x^2(n-1) + ... + b[0].                 //
fn gi_rational_polynomial(x: f64, a: &[f64], b: &[f64], n: usize) -> f64 {
    let xx: f64 = x * x;
    let mut numerator: f64 = xx + a[n - 1];
    let mut denominator: f64 = xx + b[n - 1];
    let mut i = n as i64 - 2;
    // FIXME: Convert to for
    while i >= 0 {
        numerator *= xx;
        numerator += a[i as usize];
        denominator *= xx;
        denominator += b[i as usize];
        i -= 1;
    }
    (numerator / denominator) / xx
}

//     For a large argument x, the auxiliary cosine integral, gi(x), can be   //
//     expressed as the asymptotic series                                     //
//      gi(x) ~ 1/x^2 - 6/x^4 + 120/x^6 - 5040/x^8 + ...                      //
//                                               + (2j+1)!/x^2(-x^2)^j + ...  //
fn asymptotic_series_gi(x: f64) -> f64 {
    let mut term: f64 = 1.0;
    let xx: f64 = -x * x;
    let mut xn: f64 = 1.0;
    let mut factorial: f64 = 1.0;
    let mut gi: f64 = 0.0;
    let mut old_term: f64 = 0.0;
    let mut j = 3;

    loop {
        gi += term;
        factorial *= (j * (j - 1)) as f64;
        xn *= xx;
        term = factorial / xn;
        j += 2;
        if fabsl(term) >= fabsl(old_term) {
            break;
        }
        old_term = term;
    }
    -gi / xx
}

fn xpower_series_cin(x: f64) -> f64 {
    let xx: f64 = -x * x;
    let n = (2.41 * xx + 7.15 * fabsl(x) + 7.00) as i64;
    let mut k = n + n;

    // If the argument is sufficiently small use the approximation //
    // Cin(x) = x^2 exp(-x^2/24) / 4 //

    if fabsl(x) < 0.00025 {
        return -(expl(xx / 24.0) * xx) / 4.0;
    }

    // Otherwise evaluate the power series expansion for Cin(x). //

    let mut sum = xx / (k * k * (k - 1)) as f64 + 1.0 / (k - 2) as f64;
    k -= 2;
    while k > 2 {
        sum *= xx / (k * (k - 1)) as f64;
        sum += 1.0 / (k - 2) as f64;
        k -= 2;
    }
    -xx * sum / 2.0
}

fn xpower_series_si(x: f64) -> f64 {
    let xx: f64 = -x * x;
    let n = (3.42 * xx + 7.46 * fabsl(x) + 6.95) as i64;
    let mut k = n + n;

    // If the argument is sufficiently small use the approximation //
    // Si(x) = x exp(-x^2/18) //

    if fabsl(x) <= 0.0003 {
        return x * expl(xx / 18.0);
    }

    // Otherwise evaluate the power series expansion for Si(x). //
    let mut sum = xx / ((k + 1) * (k + 1) * k) as f64 + 1.0 / (k - 1) as f64;
    k -= 1;
    while k >= 3 {
        sum *= xx / (k * (k - 1)) as f64;
        sum += 1.0 / (k - 2) as f64;
        k -= 2;
    }
    x * sum
}

//  Description:                                                              //
//     This routine returns both the auxiliary sin integral fi(x) and the     //
//     auxiliary cos integral gi(x) via the addresses in the argument list    //
//     for x > 0.  For x = 0, *fi is set to pi/2 and *gi is set to DBL_MAX.   //
fn xauxiliary_sin_cos_integrals_fi_gi(x: f64, fi: &mut f64, gi: &mut f64) {
    let si: f64;
    let ci: f64;
    let sx: f64;
    let cx: f64;

    if x == 0.0 {
        *fi = PI2;
        *gi = LDBL_MAX;
    } else if x <= 1.0 {
        si = xpower_series_si(x);
        ci = xcos_integral_ci(x);
        sx = sinl(x);
        cx = cosl(x);
        *fi = sx * ci + cx * (PI2 - si);
        *gi = sx * (PI2 - si) - cx * ci;
    } else if x <= 4.0 {
        *fi = fi_rational_polynomial(x, &a_x_ge_1_le_4_fi, &b_x_ge_1_le_4_fi, n_x_ge_1_le_4_fi);
        *gi = gi_rational_polynomial(x, &a_x_ge_1_le_4_gi, &b_x_ge_1_le_4_gi, n_x_ge_1_le_4_gi);
    } else if x <= 12.0 {
        *fi = fi_rational_polynomial(x, &a_x_ge_4_le_12_fi, &b_x_ge_4_le_12_fi, n_x_ge_4_le_12_fi);
        *gi = gi_rational_polynomial(x, &a_x_ge_4_le_12_gi, &b_x_ge_4_le_12_gi, n_x_ge_4_le_12_gi);
    } else if x < AUX_ASYMP_CUTOFF {
        *fi = fi_rational_polynomial(
            x,
            &a_x_ge_12_le_48_fi,
            &b_x_ge_12_le_48_fi,
            n_x_ge_12_le_48_fi,
        );
        *gi = gi_rational_polynomial(
            x,
            &a_x_ge_12_le_48_gi,
            &b_x_ge_12_le_48_gi,
            n_x_ge_12_le_48_gi,
        );
    } else {
        *fi = asymptotic_series_fi(x);
        *gi = asymptotic_series_gi(x);
    }
}

//     The sin integral, Si(x), is the integral with integrand                //
//                             sin(t) / t                                     //
//     where the integral extends from 0 to x.                                //
pub fn xsin_integral_si(x: f64) -> f64 {
    let mut fi: f64 = 0.0;
    let mut gi: f64 = 0.0;

    if fabsl(x) <= 1.0 {
        return xpower_series_si(x);
    }
    xauxiliary_sin_cos_integrals_fi_gi(fabsl(x), &mut fi, &mut gi);
    let si: f64 = PI2 - cosl(x) * fi - sinl(fabsl(x)) * gi;
    x.signum() * si
}

pub fn sinc_int_norm(x: f64) -> f64 {
    xsin_integral_si(x * std::f64::consts::PI) / std::f64::consts::PI
}

pub fn sinc_cumulative(x: f64, width: f64) -> f64 {
    xsin_integral_si(x * std::f64::consts::PI / width) / std::f64::consts::PI
}
